//! Raw ASCII scan of the ENTITIES section: counts every entity type, including those the dxf crate drops.

use std::collections::BTreeMap;

/// Entity type name → count inside SECTION/ENTITIES. Binary DXF → empty map.
pub fn scan_entity_types(bytes: &[u8]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return counts;
    }
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut in_entities = false;
    let mut prev_was_section = false;
    for &[code, value] in lines.as_chunks::<2>().0 {
        match code {
            "0" => {
                prev_was_section = value == "SECTION";
                if value == "ENDSEC" {
                    in_entities = false;
                } else if in_entities {
                    *counts.entry(value.to_string()).or_insert(0) += 1;
                }
            }
            "2" if prev_was_section => {
                in_entities = value == "ENTITIES";
                prev_was_section = false;
            }
            _ => prev_was_section = false,
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_entities_section() {
        let src = "0\r\nSECTION\r\n2\r\nBLOCKS\r\n0\r\nLINE\r\n0\r\nENDSEC\r\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\n0\n  0\nHATCH\n  0\nLINE\n  0\nENDSEC\n  0\nEOF\n";
        let m = scan_entity_types(src.as_bytes());
        assert_eq!(m.get("LINE"), Some(&2));
        assert_eq!(m.get("HATCH"), Some(&1));
        assert_eq!(m.len(), 2);
        assert!(scan_entity_types(b"AutoCAD Binary DXF\r\n\x1a\0").is_empty());
    }
}
