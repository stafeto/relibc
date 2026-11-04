use alloc::{string::ToString, vec::Vec};
use core::{net::Ipv4Addr, str::FromStr};

use crate::{
    error::Errno,
    out::Out,
    platform::{Pal, PalSocket, Sys},
};

use crate::header::{
    bits_arpainet::htons,
    errno::{EAGAIN, EBADMSG, EINTR, EINVAL, EIO, ETIMEDOUT},
    netinet_in::{IPPROTO_UDP, in_addr, sockaddr_in},
    poll::{POLLIN, pollfd},
    sys_socket::constants::{AF_INET, SOCK_CLOEXEC, SOCK_DGRAM},
    time::{self, timespec},
};

use super::{
    dns::{Dns, DnsAnswer, DnsQuery, valid_name},
    sys::get_dns_servers,
};

pub type LookupHost = Vec<in_addr>;

/// Attempts at the whole list of servers (`RES_DFLRETRY` 2).
const ATTEMPTS: u32 = 2;
/// Time a server gets for its answer, in milliseconds (`RES_TIMEOUT` 5 s).
const SEND_TIMEOUT_MS: i64 = 5_000;
/// Room for a reply: longer than a datagram of the network service (800
/// bytes) and than a reply without EDNS (512).
const REPLY_SIZE: usize = 1_024;

/// The socket of one lookup, closed on every path.
struct Socket(i32);

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = Sys::close(self.0);
    }
}

fn monotonic_ms() -> i64 {
    let mut now = timespec::default();
    let _ = Sys::clock_gettime(time::constants::CLOCK_MONOTONIC, Out::from_mut(&mut now));
    i64::from(now.tv_sec) * 1_000 + i64::from(now.tv_nsec) / 1_000_000
}

/// 16 random bits for the transaction number (RFC 5452, 9.2); the clock when
/// the entropy source fails.
fn transaction_id() -> u16 {
    let mut bytes = [0u8; 2];
    if matches!(Sys::getrandom(&mut bytes, 0), Ok(2)) {
        return u16::from_ne_bytes(bytes);
    }
    let mut now = timespec::default();
    let _ = Sys::clock_gettime(time::constants::CLOCK_REALTIME, Out::from_mut(&mut now));
    (now.tv_nsec >> 8) as u16
}

fn socket_address(server: u32) -> [u8; size_of::<sockaddr_in>()] {
    let mut dest = [0_u8; size_of::<sockaddr_in>()];
    *plain::from_mut_bytes(&mut dest).unwrap() = sockaddr_in {
        sin_family: u16::try_from(AF_INET).expect("AF_INET is positive and below u16::MAX"),
        sin_port: htons(53),
        sin_addr: in_addr { s_addr: server },
        ..Default::default()
    };
    dest
}

/// What one received datagram means for the question.
enum Verdict {
    /// Not an answer to this query: wait on.
    Discard,
    /// The records of the answer (an empty set is an answer too).
    Answer(Vec<DnsAnswer>),
    /// The server refuses or fails: ask the next one.
    Next,
}

/// Checks a datagram against the question (RFC 1035, 4.1.1; RFC 5452, 9.1):
/// the sender, the transaction number, the QR bit and the question.
fn judge(
    reply: &[u8],
    from: &[u8],
    servers: &[u32],
    id: u16,
    name: &str,
    q_type: u16,
) -> Result<Verdict, Errno> {
    if reply.len() < 12 || from.len() < size_of::<sockaddr_in>() {
        return Ok(Verdict::Discard);
    }
    let sender: &sockaddr_in = plain::from_bytes(&from[..size_of::<sockaddr_in>()]).unwrap();
    if sender.sin_port != htons(53) || !servers.contains(&sender.sin_addr.s_addr) {
        return Ok(Verdict::Discard);
    }
    if u16::from_be_bytes([reply[0], reply[1]]) != id || reply[2] & 0x80 == 0 {
        return Ok(Verdict::Discard);
    }
    if u16::from_be_bytes([reply[4], reply[5]]) != 1 {
        return Ok(Verdict::Discard);
    }
    let response = Dns::parse(reply).map_err(|_| Errno(EBADMSG))?;
    let asked = response.queries.first();
    let same = asked.is_some_and(|question| {
        question.name.eq_ignore_ascii_case(name)
            && question.q_type == q_type
            && question.q_class == 1
    });
    if !same {
        return Ok(Verdict::Discard);
    }
    // The code of the answer: 0 gives the records, 3 (NXDOMAIN) ends the
    // lookup at once, anything else sends the query to the next server.
    Ok(match reply[3] & 0x0F {
        0 => Verdict::Answer(response.answers),
        3 => Verdict::Answer(Vec::new()),
        _ => Verdict::Next,
    })
}

/// Asks the servers of `resolv.conf` for the records of `q_type` of `name`:
/// `ATTEMPTS` rounds, in each the servers in turn, each with `SEND_TIMEOUT_MS`.
/// Errors: `ETIMEDOUT` when nobody answered, `EAGAIN` when servers failed or
/// refused, `EBADMSG` for a reply that does not parse.
fn query(name: &str, q_type: u16) -> Result<Vec<DnsAnswer>, Errno> {
    let name = name.trim_end_matches('.');
    // A label past 63 bytes would read as a pointer to the server, and the
    // builder of the packet cannot hold one past 255.
    if !valid_name(name) {
        return Err(Errno(EINVAL));
    }
    let servers: Vec<u32> = get_dns_servers()?
        .iter()
        .filter_map(|text| Ipv4Addr::from_str(text.trim()).ok())
        .map(|address| u32::from_ne_bytes(address.octets()))
        .collect();
    if servers.is_empty() {
        return Err(Errno(EIO));
    }

    let id = transaction_id();
    let packet = Dns {
        transaction_id: id,
        flags: 0x0100,
        queries: vec![DnsQuery {
            name: name.to_string(),
            q_type,
            q_class: 0x0001,
        }],
        answers: vec![],
    }
    .compile();

    let sock = Socket(Sys::socket(
        AF_INET,
        SOCK_DGRAM | SOCK_CLOEXEC,
        i32::from(IPPROTO_UDP),
    )?);
    let mut buffer = vec![0u8; REPLY_SIZE];
    let mut failed = false;

    for _ in 0..ATTEMPTS {
        for &server in &servers {
            if Sys::sendto(sock.0, &packet, 0, Some(&socket_address(server))).is_err() {
                failed = true;
                continue;
            }
            let deadline = monotonic_ms() + SEND_TIMEOUT_MS;
            loop {
                let left = deadline - monotonic_ms();
                if left <= 0 {
                    break;
                }
                let mut wait = pollfd {
                    fd: sock.0,
                    events: POLLIN,
                    revents: 0,
                };
                // SAFETY: `wait` is one valid entry.
                let ready = unsafe { crate::header::poll::poll(&mut wait, 1, left as i32) };
                if ready < 0 {
                    if crate::platform::ERRNO.get() == EINTR {
                        continue;
                    }
                    failed = true;
                    break;
                }
                if ready == 0 {
                    break;
                }
                let mut from = [0u8; size_of::<sockaddr_in>()];
                let Ok((count, _)) = Sys::recvfrom(
                    sock.0,
                    Out::from_mut(buffer.as_mut_slice()),
                    0,
                    Some(&mut from),
                ) else {
                    failed = true;
                    break;
                };
                match judge(&buffer[..count], &from, &servers, id, name, q_type)? {
                    Verdict::Discard => {}
                    Verdict::Answer(records) => return Ok(records),
                    Verdict::Next => {
                        failed = true;
                        break;
                    }
                }
            }
        }
    }
    Err(Errno(if failed { EAGAIN } else { ETIMEDOUT }))
}

/// The IPv4 addresses `/etc/hosts` gives to `host`.
fn hosts_file(host: &str) -> Vec<in_addr> {
    use crate::{fs::File, header::fcntl, io::Read};
    let mut text = alloc::string::String::new();
    let Ok(mut file) = File::open(c"/etc/hosts".into(), fcntl::O_RDONLY) else {
        return Vec::new();
    };
    if file.read_to_string(&mut text).is_err() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut words = line.split_whitespace();
        let Some(address) = words.next().and_then(parse_ipv4_string) else {
            continue;
        };
        if words.any(|name| name.eq_ignore_ascii_case(host)) {
            found.push(in_addr { s_addr: address });
        }
    }
    found
}

pub fn lookup_host(host: &str) -> Result<LookupHost, Errno> {
    if let Some(host_direct_addr) = parse_ipv4_string(host) {
        // already an ip address
        return Ok(vec![in_addr {
            s_addr: host_direct_addr,
        }]);
    }

    // The hosts file answers before the name server is asked.
    let known = hosts_file(host);
    if !known.is_empty() {
        return Ok(known);
    }

    let answers = query(host, 0x0001)?;
    let addrs: Vec<_> = answers
        .into_iter()
        .filter_map(|answer| {
            if answer.a_type == 0x0001 && answer.a_class == 0x0001 && answer.data.len() == 4 {
                let addr = in_addr {
                    s_addr: u32::from_ne_bytes([
                        answer.data[0],
                        answer.data[1],
                        answer.data[2],
                        answer.data[3],
                    ]),
                };
                Some(addr)
            } else {
                None
            }
        })
        .collect();

    Ok(addrs)
}

/// The names `/etc/hosts` gives to the IPv4 address `addr`.
fn hosts_names(addr: &in_addr) -> Vec<Vec<u8>> {
    use crate::{fs::File, header::fcntl, io::Read};
    let mut text = alloc::string::String::new();
    let Ok(mut file) = File::open(c"/etc/hosts".into(), fcntl::O_RDONLY) else {
        return Vec::new();
    };
    if file.read_to_string(&mut text).is_err() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut words = line.split_whitespace();
        if words.next().and_then(parse_ipv4_string) == Some(addr.s_addr) {
            found.extend(words.map(|name| name.as_bytes().to_vec()));
        }
    }
    found
}

pub fn lookup_addr(addr: &in_addr) -> Result<Vec<Vec<u8>>, Errno> {
    // The hosts file answers before the name server is asked.
    let known = hosts_names(addr);
    if !known.is_empty() {
        return Ok(known);
    }
    let addr: [u8; 4] = addr.s_addr.to_ne_bytes();
    // Address intentionally backwards for reverse lookup
    let name = format!(
        "{}.{}.{}.{}.in-addr.arpa",
        addr[3], addr[2], addr[1], addr[0]
    );
    let names = query(&name, 0x000C)?
        .into_iter()
        .filter_map(|answer| {
            if answer.a_type == 0x000C && answer.a_class == 0x0001 {
                // answer.data is encoded kinda weird.
                // Basically length-prefixed strings for each
                // subsection of the domain.
                // We need to parse this to insert periods where
                // they belong (ie at the end of each string)
                Some(parse_revdns_answer(&answer.data))
            } else {
                None
            }
        })
        .collect();
    Ok(names)
}

fn parse_revdns_answer(data: &[u8]) -> Vec<u8> {
    if data.is_empty() || data[0] == 0 {
        return vec![0];
    }

    let mut cursor = 0;
    let mut index = 0;
    // First byte is a length; discard
    let mut output = data[1..].to_vec();
    let length = data.len() - 1;
    while index < length {
        let offset = data[index] as usize;
        // CVE-2024-21342
        if offset > length {
            return vec![0];
        }
        index = cursor + offset + 1;
        // First byte was skipped so index is one less
        output[index - 1] = b'.';
        cursor = index;
    }
    // Response is NUL terminated so we must preserve that for C
    match output.last_mut() {
        Some(nul) => *nul = b'\0',
        // XXX: Likely unreachable
        None => {
            debug_assert!(output.is_empty());
            output = vec![0];
        }
    }
    output
}

pub fn parse_ipv4_string(ip_string: &str) -> Option<u32> {
    let dns_vec: Vec<u8> = ip_string
        .trim()
        .split('.')
        .map(|octet| octet.parse::<u8>().unwrap_or(0))
        .collect();

    if dns_vec.len() != 4 {
        return None;
    }

    let mut dns_arr = [0u8; 4];

    for (i, octet) in dns_vec.iter().enumerate() {
        dns_arr[i] = *octet;
    }

    Some(u32::from_ne_bytes(dns_arr))
}

#[cfg(test)]
mod tests {
    use alloc::str;
    use core::ffi::CStr;

    use super::parse_revdns_answer;

    // Actual response from a query
    const DNS_GOOGLE: &[u8] = &[3, 100, 110, 115, 6, 103, 111, 111, 103, 108, 101, 0];
    const EXPECTED_DNS_GOOGLE: &str = "dns.google\0";
    const EXPECTED_DNS_GOOGLE_RT: &str = "dns.google";

    // Fake response that has numbers within the name (e.g. like CDNs)
    const FAKE_WITH_NUMS: &[u8] = &[
        14, 102, 97, 107, 101, 45, 49, 48, 48, 45, 50, 45, 51, 45, 52, 8, 102, 111, 111, 98, 97,
        114, 50, 52, 4, 102, 97, 107, 101, 0,
    ];
    const EXPECTED_FAKE_RESPONSE: &str = "fake-100-2-3-4.foobar24.fake\0";
    const EXPECTED_FAKE_RESPONSE_RT: &str = "fake-100-2-3-4.foobar24.fake";

    const EMPTY_RESPONSE: &[u8] = &[0];
    const EXPECTED_EMPTY_RESPONSE: &str = "\0";
    const EXPECTED_EMPTY_RESPONSE_RT: &str = "";

    #[test]
    fn dns_response_dns_google() {
        let response = parse_revdns_answer(DNS_GOOGLE);
        assert_eq!(
            0,
            *response.last().unwrap(),
            "Response should end with a NUL byte"
        );

        let response_str = str::from_utf8(&response)
            .expect("Response is valid UTF-8; parsing shouldn't change that");
        assert_eq!(EXPECTED_DNS_GOOGLE, response_str);

        let response_cstr = CStr::from_bytes_with_nul(&response)
            .expect("Parsed response should have only one NUL byte");
        let response_cstr_str = response_cstr
            .to_str()
            .expect("Valid UTF-8 bytes to CStr to Rust str should be valid");
        assert_eq!(EXPECTED_DNS_GOOGLE_RT, response_cstr_str);
    }

    #[test]
    fn dns_response_fake_with_nums() {
        let response = parse_revdns_answer(FAKE_WITH_NUMS);
        assert_eq!(
            0,
            *response.last().unwrap(),
            "Response should end with a NUL byte"
        );

        let response_str = str::from_utf8(&response)
            .expect("Response is valid UTF-8; parsing shouldn't change that");
        assert_eq!(EXPECTED_FAKE_RESPONSE, response_str);

        let response_cstr = CStr::from_bytes_with_nul(&response)
            .expect("Parsed response should have only one NUL byte");
        let response_cstr_str = response_cstr
            .to_str()
            .expect("Valid UTF-8 bytes to CStr to Rust str should be valid");
        assert_eq!(EXPECTED_FAKE_RESPONSE_RT, response_cstr_str);
    }

    #[test]
    fn dns_response_empty() {
        let response = parse_revdns_answer(EMPTY_RESPONSE);
        assert_eq!(
            0,
            *response.last().unwrap(),
            "Response should end with a NUL byte"
        );

        let response_str = str::from_utf8(&response)
            .expect("Response is valid UTF-8; parsing shouldn't change that");
        assert_eq!(EXPECTED_EMPTY_RESPONSE, response_str);

        let response_cstr = CStr::from_bytes_with_nul(&response)
            .expect("Parsed response should have only one NUL byte");
        let response_cstr_str = response_cstr
            .to_str()
            .expect("Valid UTF-8 bytes to CStr to Rust str should be valid");
        assert_eq!(EXPECTED_EMPTY_RESPONSE_RT, response_cstr_str);
    }
}
