//! Binary version triplet with manifest compatibility.
//!
//! Every version stamp in confit uses one triplet that moves with the binary.

/// The binary version triplet.
pub const VERSION: [u8; 3] = triplet(env!("CARGO_PKG_VERSION").as_bytes());

/// Returns whether one manifest version loads under this binary.
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
        panic!("expected version parts under 256");
    }
    value as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_loads_its_own_stamp() {
        assert!(compatible(VERSION), "the binary loads its own stamp");
    }

    #[test]
    fn loads_accepts_fix_drift() {
        assert!(loads([0, 9, 4], [0, 9, 7]), "the loader accepts fix drift");
    }

    #[test]
    fn loads_rejects_minor_drift_before_one() {
        assert!(
            !loads([0, 9, 0], [0, 10, 0]),
            "the loader rejects minor drift before 1.0.0"
        );
    }

    #[test]
    fn loads_accepts_minor_drift_past_one() {
        assert!(
            loads([1, 4, 0], [1, 7, 2]),
            "the loader accepts minor drift past 1.0.0"
        );
    }

    #[test]
    fn loads_rejects_major_drift() {
        assert!(
            !loads([0, 9, 0], [1, 0, 0]),
            "the loader rejects major drift"
        );
        assert!(
            !loads([1, 0, 0], [2, 0, 0]),
            "the loader rejects major drift"
        );
    }
}
