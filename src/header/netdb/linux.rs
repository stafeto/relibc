use crate::{
    error::Errno,
    fs::File,
    header::{errno, fcntl},
    io::Read,
};
use alloc::{string::String, vec::Vec};

/// Name servers kept from `resolv.conf` (`MAXNS` of glibc and musl).
const MAX_SERVERS: usize = 3;

/// The addresses of the first three `nameserver` lines of `text`. Blank
/// lines and lines starting with `#` or `;` are skipped, so are the
/// `search`, `domain` and `options` lines.
pub fn parse_nameservers(text: &str) -> Vec<String> {
    let mut servers = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let mut words = line.split_whitespace();
        if words.next() != Some("nameserver") {
            continue;
        }
        if let Some(address) = words.next() {
            servers.push(address.into());
            if servers.len() == MAX_SERVERS {
                break;
            }
        }
    }
    servers
}

pub fn get_dns_servers() -> Result<Vec<String>, Errno> {
    let mut text = String::new();
    let mut file = File::open(c"/etc/resolv.conf".into(), fcntl::O_RDONLY)?;
    file.read_to_string(&mut text)
        .map_err(|_| Errno(errno::EIO).sync())?;
    let servers = parse_nameservers(&text);
    if servers.is_empty() {
        return Err(Errno(errno::EIO).sync());
    }
    Ok(servers)
}

#[cfg(test)]
mod tests {
    use super::parse_nameservers;

    #[test]
    fn the_first_three_nameserver_lines_count() {
        let text = "# c\n; c\nsearch x\n  nameserver\t10.0.0.1  \nnameserver\nnameserver ::1\nnameserver 10.0.0.2\nnameserver 10.0.0.3\nnameserver 10.0.0.4\n";
        assert_eq!(parse_nameservers(text), ["10.0.0.1", "::1", "10.0.0.2"]);
    }

    #[test]
    fn a_word_that_only_starts_with_nameserver_is_no_line() {
        assert!(parse_nameservers("nameserverx 10.0.0.1\n").is_empty());
    }
}
