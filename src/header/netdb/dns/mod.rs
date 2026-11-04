// Copyright 2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use self::{answer::DnsAnswer, query::DnsQuery};

use alloc::{string::String, vec::Vec};

mod answer;
mod query;

#[derive(Clone, Debug)]
pub struct Dns {
    pub transaction_id: u16,
    pub flags: u16,
    pub queries: Vec<DnsQuery>,
    pub answers: Vec<DnsAnswer>,
}

impl Dns {
    pub fn compile(&self) -> Vec<u8> {
        let mut data = Vec::new();

        macro_rules! push_u8 {
            ($value:expr) => {
                data.push($value);
            };
        }

        macro_rules! push_n16 {
            ($value:expr) => {
                data.extend_from_slice(&u16::to_be_bytes($value));
            };
        }

        push_n16!(self.transaction_id);
        push_n16!(self.flags);
        push_n16!(u16::try_from(self.queries.len()).expect("shouldn't exceed u16::MAX"));
        push_n16!(u16::try_from(self.answers.len()).expect("shouldn't exceed u16::MAX"));
        push_n16!(0);
        push_n16!(0);

        for query in self.queries.iter() {
            for part in query.name.split('.') {
                push_u8!(u8::try_from(part.len()).expect("shouldn't exceed u8::MAX"));
                data.extend_from_slice(part.as_bytes());
            }
            push_u8!(0);
            push_n16!(query.q_type);
            push_n16!(query.q_class);
        }
        data
    }

    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let mut i = 0;

        macro_rules! pop_u8 {
            () => {{
                i += 1;
                if i > data.len() {
                    return Err(format!("{}: {}: pop_u8", file!(), line!()));
                }
                data[i - 1]
            }};
        }

        macro_rules! pop_n16 {
            () => {{
                use core::convert::TryInto;
                i += 2;
                if i > data.len() {
                    return Err(format!("{}: {}: pop_n16", file!(), line!()));
                }
                let bytes: [u8; 2] = data[i - 2..i].try_into().unwrap();
                u16::from_be_bytes(bytes)
            }};
        }

        macro_rules! pop_data {
            () => {{
                let mut data = Vec::new();

                let data_len = pop_n16!();
                for _data_i in 0..data_len {
                    data.push(pop_u8!());
                }

                data
            }};
        }

        macro_rules! pop_name {
            () => {{
                let (name, next) = read_name(data, i)?;
                i = next;
                name
            }};
        }

        let transaction_id = pop_n16!();
        let flags = pop_n16!();
        let queries_len = pop_n16!();
        let answers_len = pop_n16!();
        let _ = pop_n16!();
        let _ = pop_n16!();

        let mut queries = Vec::new();
        for _query_i in 0..queries_len {
            queries.push(DnsQuery {
                name: pop_name!(),
                q_type: pop_n16!(),
                q_class: pop_n16!(),
            });
        }

        let mut answers = Vec::new();
        for _answer_i in 0..answers_len {
            answers.push(DnsAnswer {
                name: pop_name!(),
                a_type: pop_n16!(),
                a_class: pop_n16!(),
                ttl_a: pop_n16!(),
                ttl_b: pop_n16!(),
                data: pop_data!(),
            });
        }

        Ok(Dns {
            transaction_id,
            flags,
            queries,
            answers,
        })
    }
}

/// Longest jump chain followed in a name.
const MAX_JUMPS: usize = 16;
/// Longest name (RFC 1035, 2.3.4).
const MAX_NAME: usize = 255;

/// Reads the name at `start`: labels, ending in a zero byte or in a
/// compression pointer (RFC 1035, 4.1.4). Returns the name and the position
/// after it. A pointer goes back only, to a place before the byte of the
/// pointer itself, and at most `MAX_JUMPS` pointers are followed, so that a
/// reply cannot make the parse loop.
fn read_name(data: &[u8], start: usize) -> Result<(String, usize), String> {
    let mut name = String::new();
    let mut pos = start;
    let mut resume = None;
    let mut jumps = 0;
    loop {
        let len = *data
            .get(pos)
            .ok_or_else(|| String::from("name: end of data"))?;
        match len & 0b1100_0000 {
            0b1100_0000 => {
                let low = *data
                    .get(pos + 1)
                    .ok_or_else(|| String::from("name: short pointer"))?;
                let target = (usize::from(len & 0b0011_1111) << 8) | usize::from(low);
                if resume.is_none() {
                    resume = Some(pos + 2);
                }
                jumps += 1;
                if jumps > MAX_JUMPS || target >= pos {
                    return Err(String::from("name: bad pointer"));
                }
                pos = target;
            }
            0 => {
                let len = usize::from(len);
                if len == 0 {
                    return Ok((name, resume.unwrap_or(pos + 1)));
                }
                let label = data
                    .get(pos + 1..pos + 1 + len)
                    .ok_or_else(|| String::from("name: short label"))?;
                if !name.is_empty() {
                    name.push('.');
                }
                if name.len() + len > MAX_NAME {
                    return Err(String::from("name: too long"));
                }
                name.extend(label.iter().map(|&byte| byte as char));
                pos += 1 + len;
            }
            _ => return Err(String::from("name: reserved label type")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Dns, read_name};

    #[test]
    fn a_pointer_to_itself_ends_with_an_error() {
        let mut data = [0u8; 14];
        data[12] = 0xC0;
        data[13] = 12;
        assert!(read_name(&data, 12).is_err());
    }

    #[test]
    fn labels_then_a_pointer_make_one_name() {
        let mut data = vec![0u8; 12];
        data.extend_from_slice(&[4, b't', b'e', b's', b't', 0]);
        let at = data.len();
        data.extend_from_slice(&[1, b'a', 0xC0, 12]);
        assert_eq!(read_name(&data, at), Ok(("a.test".into(), at + 4)));
    }

    #[test]
    fn a_reply_with_a_looping_answer_does_not_parse() {
        let mut data = vec![0, 1, 0x81, 0x80, 0, 0, 0, 1, 0, 0, 0, 0];
        data.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert!(Dns::parse(&data).is_err());
    }
}
