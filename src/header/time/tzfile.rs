//! Time zones from TZif files (RFC 9636) and POSIX `TZ` strings.
//!
//! A zone is read once from `/usr/share/zoneinfo/<name>` (or the absolute
//! path in `TZ`) and kept for the life of the process. The 64-bit block of the
//! file gives the transitions; the footer rule gives the years after the last
//! one. A `TZ` value with no file is read as a POSIX rule
//! (`EST5EDT,M3.2.0,M11.1.0`). `UTC` needs no file.

use crate::{
    c_str::CString,
    fs::File,
    header::fcntl::O_RDONLY,
    io::Read,
    sync::Mutex,
};
use alloc::{string::String, vec::Vec};
use chrono::{FixedOffset, NaiveDate, NaiveDateTime, Offset, TimeZone, offset::MappedLocalTime};

const ZONEINFO: &str = "/usr/share/zoneinfo/";
const MAX_FILE: usize = 1 << 17;
const DAY: i64 = 86400;

struct TType {
    utoff: i32,
    isdst: bool,
    abbr: String,
}

#[derive(Clone, Copy)]
enum Day {
    /// `Mm.w.d`: day `d` of week `w` (5 is the last) of month `m`.
    Month(u8, u8, u8),
    /// `Jn`: day 1 to 365, February 29 not counted.
    Julian(u16),
    /// `n`: day 0 to 365, February 29 counted.
    Zero(u16),
}

#[derive(Clone, Copy)]
struct Change {
    day: Day,
    /// Seconds after local midnight; may be negative or exceed a day.
    time: i32,
}

struct Rule {
    std: TType,
    dst: Option<(TType, Change, Change)>,
}

pub struct Zone {
    name: String,
    transitions: Vec<i64>,
    kinds: Vec<u8>,
    types: Vec<TType>,
    rule: Option<Rule>,
}

#[derive(Clone, Copy)]
pub struct Tz(&'static Zone);

#[derive(Clone, Copy)]
pub struct TzOffset {
    zone: &'static Zone,
    utoff: i32,
    isdst: bool,
    abbr: &'static str,
}

impl core::fmt::Debug for TzOffset {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}{:+}", self.abbr, self.utoff)
    }
}

impl Offset for TzOffset {
    fn fix(&self) -> FixedOffset {
        FixedOffset::east_opt(self.utoff).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap())
    }
}

impl TzOffset {
    pub fn abbreviation(&self) -> &'static str {
        self.abbr
    }

    pub fn is_dst(&self) -> bool {
        self.isdst
    }
}

impl TimeZone for Tz {
    type Offset = TzOffset;

    fn from_offset(offset: &TzOffset) -> Tz {
        Tz(offset.zone)
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<TzOffset> {
        self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<TzOffset> {
        let seconds = local.and_utc().timestamp();
        let mut found: [Option<(i64, TzOffset)>; 2] = [None, None];
        let mut count = 0;
        // The offsets of the day before and the day after hold every offset
        // the local time can have.
        let around = [seconds - DAY, seconds, seconds + DAY].map(|t| self.0.lookup(t));
        for (i, candidate) in around.iter().enumerate() {
            if around[..i].iter().any(|o| o.utoff == candidate.utoff) {
                continue;
            }
            let at = seconds - i64::from(candidate.utoff);
            if self.0.lookup(at).utoff == candidate.utoff && count < 2 {
                found[count] = Some((at, *candidate));
                count += 1;
            }
        }
        match (found[0], found[1]) {
            (Some((_, a)), None) => MappedLocalTime::Single(a),
            (Some((ta, a)), Some((tb, b))) => {
                if ta <= tb {
                    MappedLocalTime::Ambiguous(a, b)
                } else {
                    MappedLocalTime::Ambiguous(b, a)
                }
            }
            _ => MappedLocalTime::None,
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> TzOffset {
        self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> TzOffset {
        self.0.lookup(utc.and_utc().timestamp())
    }
}

impl Tz {
    pub fn utc() -> Tz {
        static UTC: Mutex<Option<&'static Zone>> = Mutex::new(None);
        let mut slot = UTC.lock();
        Tz(*slot.get_or_insert_with(|| leak(builtin_utc("UTC"))))
    }

    /// The zone `name` (`TZ` as set), or `None` if neither a file nor a
    /// POSIX rule gives it.
    pub fn named(name: &str) -> Option<Tz> {
        static CACHE: Mutex<Vec<(String, Option<Tz>)>> = Mutex::new(Vec::new());
        let mut cache = CACHE.lock();
        if let Some((_, zone)) = cache.iter().find(|(n, _)| n == name) {
            return *zone;
        }
        let zone = Self::load(name);
        cache.push((String::from(name), zone));
        zone
    }

    fn load(name: &str) -> Option<Tz> {
        if name.is_empty() || name == "UTC" {
            return Some(Self::utc());
        }
        let file = name.strip_prefix(':').unwrap_or(name);
        let zone = read_file(file)
            .and_then(|bytes| Zone::parse(file, &bytes))
            .or_else(|| Zone::from_posix(name))?;
        Some(Tz(leak(zone)))
    }

    /// The abbreviation and the offset of the standard time, and the
    /// abbreviation of the daylight saving time if the zone has one.
    pub fn names(&self) -> (&'static str, i32, Option<&'static str>) {
        let zone = self.0;
        if let Some(rule) = &zone.rule {
            return (
                &rule.std.abbr,
                rule.std.utoff,
                rule.dst.as_ref().map(|(t, _, _)| t.abbr.as_str()),
            );
        }
        let used = |dst: bool| {
            zone.kinds
                .iter()
                .rev()
                .map(|&k| &zone.types[usize::from(k)])
                .find(|t| t.isdst == dst)
        };
        let std = used(false)
            .or_else(|| zone.types.iter().find(|t| !t.isdst))
            .or(zone.types.first());
        let dst = used(true);
        match std {
            Some(t) => (&t.abbr, t.utoff, dst.map(|d| d.abbr.as_str())),
            None => ("UTC", 0, None),
        }
    }
}

fn leak(zone: Zone) -> &'static Zone {
    alloc::boxed::Box::leak(alloc::boxed::Box::new(zone))
}

fn builtin_utc(name: &str) -> Zone {
    Zone {
        name: String::from(name),
        transitions: Vec::new(),
        kinds: Vec::new(),
        types: alloc::vec![TType {
            utoff: 0,
            isdst: false,
            abbr: String::from("UTC"),
        }],
        rule: None,
    }
}

fn read_file(name: &str) -> Option<Vec<u8>> {
    if name.is_empty() || name.contains("..") || name.contains('\0') {
        return None;
    }
    let path = if name.starts_with('/') {
        String::from(name)
    } else {
        let mut p = String::from(ZONEINFO);
        p.push_str(name);
        p
    };
    let path = CString::new(path).ok()?;
    let mut file = File::open(path.as_c_str().into(), O_RDONLY).ok()?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = file.read(&mut chunk).ok()?;
        if n == 0 {
            return Some(bytes);
        }
        if bytes.len() + n > MAX_FILE {
            return None;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

impl Zone {
    /// A TZif file: the 64-bit block of version 2 and later, the 32-bit
    /// block of version 1.
    fn parse(name: &str, b: &[u8]) -> Option<Zone> {
        if b.get(..4)? != b"TZif" {
            return None;
        }
        let version = *b.get(4)?;
        let counts = |at: usize| -> Option<[usize; 6]> {
            let mut c = [0usize; 6];
            for (i, slot) in c.iter_mut().enumerate() {
                *slot = be32(b, at + 20 + 4 * i)? as usize;
            }
            Some(c)
        };
        // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt
        let c1 = counts(0)?;
        let block = |c: &[usize; 6], size: usize| {
            c[3] * size + c[3] + c[4] * 6 + c[5] + c[2] * (size + 4) + c[1] + c[0]
        };
        let (start, c, size) = if version >= b'2' {
            let second = 44 + block(&c1, 4);
            (second + 44, counts(second)?, 8)
        } else {
            (44, c1, 4)
        };
        let [_isut, _isstd, leap, timecnt, typecnt, charcnt] = c;
        if typecnt == 0 {
            return None;
        }
        let mut at = start;
        let mut transitions = Vec::with_capacity(timecnt);
        for i in 0..timecnt {
            transitions.push(if size == 8 {
                be64(b, at + 8 * i)?
            } else {
                i64::from(be32(b, at + 4 * i)? as i32)
            });
        }
        at += timecnt * size;
        let kinds = b.get(at..at + timecnt)?.to_vec();
        at += timecnt;
        let descriptors = b.get(at..at + typecnt * 6)?;
        at += typecnt * 6;
        let chars = b.get(at..at + charcnt)?;
        at += charcnt;
        let mut types = Vec::with_capacity(typecnt);
        for d in descriptors.chunks(6) {
            let utoff = i32::from_be_bytes(d[..4].try_into().ok()?);
            let tail = chars.get(usize::from(d[5])..)?;
            let end = tail.iter().position(|&c| c == 0)?;
            types.push(TType {
                utoff,
                isdst: d[4] != 0,
                abbr: String::from_utf8(tail[..end].to_vec()).ok()?,
            });
        }
        if kinds.iter().any(|&k| usize::from(k) >= typecnt) {
            return None;
        }
        let mut rule = None;
        if size == 8 {
            at += leap * (size + 4) + c[1] + c[0];
            let footer = b.get(at..)?;
            if let Some(text) = footer.strip_prefix(b"\n") {
                let end = text.iter().position(|&c| c == b'\n')?;
                if end > 0 {
                    rule = Rule::parse(core::str::from_utf8(&text[..end]).ok()?);
                }
            }
        }
        Some(Zone {
            name: String::from(name),
            transitions,
            kinds,
            types,
            rule,
        })
    }

    fn from_posix(text: &str) -> Option<Zone> {
        Some(Zone {
            name: String::from(text),
            transitions: Vec::new(),
            kinds: Vec::new(),
            types: Vec::new(),
            rule: Some(Rule::parse(text)?),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn lookup(&'static self, t: i64) -> TzOffset {
        let make = |k: &'static TType| TzOffset {
            zone: self,
            utoff: k.utoff,
            isdst: k.isdst,
            abbr: &k.abbr,
        };
        if let Some(&last) = self.transitions.last() {
            if t < self.transitions[0] {
                let first = self
                    .types
                    .iter()
                    .find(|k| !k.isdst)
                    .unwrap_or(&self.types[0]);
                return make(first);
            }
            if t < last || self.rule.is_none() {
                let n = self.transitions.partition_point(|&x| x <= t);
                return make(&self.types[usize::from(self.kinds[n - 1])]);
            }
        } else if self.rule.is_none() {
            return make(&self.types[0]);
        }
        let rule = self.rule.as_ref().unwrap();
        make(rule.kind_at(t))
    }
}

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn year_of(days: i64) -> i64 {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    if mp < 10 { y } else { y + 1 }
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_days(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Change {
    /// Seconds since the epoch of the local time of this change in `year`,
    /// as if the local time were UTC.
    fn local_seconds(&self, year: i64) -> i64 {
        let day = match self.day {
            Day::Month(m, w, d) => {
                let (m, w, d) = (i64::from(m), i64::from(w), i64::from(d));
                let first = days_from_civil(year, m, 1);
                let weekday = (first + 4).rem_euclid(7);
                let mut day = 1 + (d - weekday).rem_euclid(7) + (w - 1) * 7;
                while day > month_days(year, m) {
                    day -= 7;
                }
                first + day - 1
            }
            Day::Julian(n) => {
                let n = i64::from(n);
                let leap = i64::from(is_leap(year) && n >= 60);
                days_from_civil(year, 1, 1) + n - 1 + leap
            }
            Day::Zero(n) => days_from_civil(year, 1, 1) + i64::from(n),
        };
        day * DAY + i64::from(self.time)
    }
}

impl Rule {
    fn kind_at(&self, t: i64) -> &TType {
        let Some((dst, start, end)) = &self.dst else {
            return &self.std;
        };
        let year = year_of((t + i64::from(self.std.utoff)).div_euclid(DAY));
        let begins = start.local_seconds(year) - i64::from(self.std.utoff);
        let ends = end.local_seconds(year) - i64::from(dst.utoff);
        let in_dst = if begins < ends {
            begins <= t && t < ends
        } else {
            !(ends <= t && t < begins)
        };
        if in_dst { dst } else { &self.std }
    }

    /// A POSIX `TZ` value: `std offset [dst [offset] [,start[/time],end[/time]]]`.
    fn parse(text: &str) -> Option<Rule> {
        let mut s = text;
        let std = name(&mut s)?;
        let std_off = -offset(&mut s)?;
        let std = TType {
            utoff: std_off,
            isdst: false,
            abbr: std,
        };
        if s.is_empty() {
            return Some(Rule { std, dst: None });
        }
        let dst_name = name(&mut s)?;
        let dst_off = if s.is_empty() || s.starts_with(',') {
            std_off + 3600
        } else {
            -offset(&mut s)?
        };
        let (start, end) = match s.strip_prefix(',') {
            Some(rest) => {
                let (a, b) = rest.split_once(',')?;
                (change(a)?, change(b)?)
            }
            // The rule of the United States when none is given.
            None if s.is_empty() => (change("M3.2.0")?, change("M11.1.0")?),
            None => return None,
        };
        let dst = TType {
            utoff: dst_off,
            isdst: true,
            abbr: dst_name,
        };
        Some(Rule {
            std,
            dst: Some((dst, start, end)),
        })
    }
}

fn name(s: &mut &str) -> Option<String> {
    let text = *s;
    if let Some(rest) = text.strip_prefix('<') {
        let end = rest.find('>')?;
        *s = &rest[end + 1..];
        return Some(String::from(&rest[..end])).filter(|n| n.len() >= 3);
    }
    let end = text
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    if end < 3 {
        return None;
    }
    *s = &text[end..];
    Some(String::from(&text[..end]))
}

/// `[+-]hh[:mm[:ss]]` in seconds; the sign is the one of the text.
fn offset(s: &mut &str) -> Option<i32> {
    let text = *s;
    let (sign, rest) = match text.as_bytes().first()? {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => (1, text),
    };
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':'))
        .unwrap_or(rest.len());
    let mut total = 0i32;
    for (i, part) in rest[..end].split(':').enumerate() {
        if i > 2 {
            return None;
        }
        let n: i32 = part.parse().ok()?;
        total += n * [3600, 60, 1][i];
    }
    if end == 0 || total > 24 * 3600 * 7 {
        return None;
    }
    *s = &rest[end..];
    Some(sign * total)
}

fn change(text: &str) -> Option<Change> {
    let (date, time) = match text.split_once('/') {
        Some((d, t)) => {
            let mut t = t;
            let time = offset(&mut t)?;
            if !t.is_empty() {
                return None;
            }
            (d, time)
        }
        None => (text, 7200),
    };
    let day = if let Some(m) = date.strip_prefix('M') {
        let mut parts = m.split('.');
        let month: u8 = parts.next()?.parse().ok()?;
        let week: u8 = parts.next()?.parse().ok()?;
        let wday: u8 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=5).contains(&week) || wday > 6
        {
            return None;
        }
        Day::Month(month, week, wday)
    } else if let Some(j) = date.strip_prefix('J') {
        let n: u16 = j.parse().ok()?;
        if !(1..=365).contains(&n) {
            return None;
        }
        Day::Julian(n)
    } else {
        let n: u16 = date.parse().ok()?;
        if n > 365 {
            return None;
        }
        Day::Zero(n)
    };
    Some(Change { day, time })
}

impl Tz {
    pub fn name(&self) -> &str {
        self.0.name()
    }
}
