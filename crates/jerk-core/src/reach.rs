use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub const REFUSED: &str = "this address is refused over the link";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Public,
    Loopback,
}

impl Reach {
    pub fn of_start(url: &str) -> Reach {
        let loopback = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(jerk_oauth::config::is_loopback_host));
        if loopback == Some(true) {
            Reach::Loopback
        } else {
            Reach::Public
        }
    }

    pub fn allows(self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        match (self, ip) {
            (Reach::Loopback, _) => ip.is_loopback(),
            (Reach::Public, IpAddr::V4(v4)) => public_v4(v4),
            (Reach::Public, IpAddr::V6(v6)) => public_v6(v6),
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let reserved = a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (18..20).contains(&b));
    !(reserved
        || ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_documentation())
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let [first, second, third, fourth, fifth, sixth, ..] = ip.segments();
    let [.., p, q, r, s] = ip.octets();
    if [first, second, third, fourth, fifth, sixth] == [0x0064, 0xff9b, 0, 0, 0, 0] {
        return public_v4(Ipv4Addr::new(p, q, r, s));
    }
    if first == 0x2002 {
        let [a, b] = second.to_be_bytes();
        let [c, d] = third.to_be_bytes();
        return public_v4(Ipv4Addr::new(a, b, c, d));
    }
    let global = first & 0xe000 == 0x2000;
    let special = first == 0x2001 && second < 0x0200;
    let documentation =
        (first == 0x2001 && second == 0x0db8) || (first == 0x3fff && second < 0x1000);
    global && !special && !documentation
}
