//! Binary version triplet with manifest compatibility.

//! Every version stamp in confit reads one triplet moving with the binary.

/// Binary version triplet: major, minor, fix.
pub const VERSION: [u8; 3] = triplet(env!("CARGO_PKG_VERSION").as_bytes());

/// Report whether one manifest version loads under this binary.
pub fn compatible(manifest: [u8; 3]) -> bool {
    loads(VERSION, manifest)
}

fn loads(binary: [u8; 3], manifest: [u8; 3]) -> bool {
    manifest[0] == binary[0] && (binary[0] > 0 || manifest[1] == binary[1])
}

const fn triplet(raw: &[u8]) -> [u8; 3] {
    let mut out = [0u8; 3];
    let mut part = 0usize;
    let mut value = 0u16;
    let mut index = 0usize;
    while index < raw.len() {
        let byte = raw[index];
        if byte == b'.' {
            if part >= 2 {
                panic!("confit version holds three parts");
            }
            out[part] = narrow(value);
            part += 1;
            value = 0;
        } else if byte >= b'0' && byte <= b'9' {
            value = value * 10 + (byte - b'0') as u16;
        } else {
            panic!("confit version holds digits and dots alone");
        }
        index += 1;
    }
    if part != 2 {
        panic!("confit version holds three parts");
    }
    out[part] = narrow(value);
    out
}

const fn narrow(value: u16) -> u8 {
    if value > 255 {
        panic!("confit version parts stay under 256");
    }
    value as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_triplet_loads() {
        assert!(compatible(VERSION), "the binary loads its own stamp");
    }

    #[test]
    fn fix_drift_loads() {
        assert!(loads([0, 9, 4], [0, 9, 7]), "fix drift never blocks");
    }

    #[test]
    fn minor_drift_refuses_before_one() {
        assert!(!loads([0, 9, 0], [0, 10, 0]), "zero majors break per minor");
    }

    #[test]
    fn minor_drift_loads_past_one() {
        assert!(loads([1, 4, 0], [1, 7, 2]), "stable majors ignore minors");
    }

    #[test]
    fn major_drift_refuses() {
        assert!(!loads([0, 9, 0], [1, 0, 0]), "majors always gate");
        assert!(!loads([1, 0, 0], [2, 0, 0]), "majors always gate");
    }
}
