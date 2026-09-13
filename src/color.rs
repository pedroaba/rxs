//! Platform-independent sRGB color editing and palette serialization.
pub type Rgba = [f64; 4];

pub fn hsv(rgb: Rgba, previous_hue: f64) -> [f64; 3] {
    let [r, g, b, _] = rgb;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta < 1e-12 {
        previous_hue
    } else if max == r {
        (60.0 * ((g - b) / delta)).rem_euclid(360.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    [h, if max == 0.0 { 0.0 } else { delta / max }, max]
}

pub fn rgb([h, s, v]: [f64; 3], alpha: f64) -> Rgba {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let [r, g, b] = match h as u32 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    let m = v - c;
    [r + m, g + m, b + m, alpha]
}

pub fn hex(color: Rgba, include_alpha: bool) -> String {
    color[..if include_alpha { 4 } else { 3 }]
        .iter()
        .map(|v| format!("{:02X}", (v.clamp(0.0, 1.0) * 255.0).round() as u8))
        .collect()
}

pub fn parse_hex(text: &str, alpha: f64) -> Option<Rgba> {
    let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&text[0..2], 16).ok()? as f64 / 255.0,
        u8::from_str_radix(&text[2..4], 16).ok()? as f64 / 255.0,
        u8::from_str_radix(&text[4..6], 16).ok()? as f64 / 255.0,
        alpha,
    ])
}

pub fn encode_palette(colors: &[Rgba]) -> String {
    colors
        .iter()
        .map(|c| hex(*c, true))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn decode_palette(text: &str) -> Vec<Rgba> {
    let mut colors = Vec::new();
    for entry in text.split(',') {
        if entry.len() != 8 || !entry.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let (Some(mut c), Ok(a)) = (
            parse_hex(&entry[..6], 1.0),
            u8::from_str_radix(&entry[6..], 16),
        ) {
            c[3] = a as f64 / 255.0;
            if !colors.contains(&c) {
                colors.push(c);
            }
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_achromatic_hue() {
        for c in [
            [1.0, 0.0, 0.0, 0.3],
            [0.2, 0.7, 0.9, 1.0],
            [0.0; 4],
            [1.0; 4],
        ] {
            let result = rgb(hsv(c, 217.0), c[3]);
            for i in 0..4 {
                assert!((result[i] - c[i]).abs() < 1e-10);
            }
        }
        assert_eq!(hsv([0.5, 0.5, 0.5, 1.0], 217.0), [217.0, 0.0, 0.5]);
        assert_eq!(rgb([360.0, 1.0, 1.0], 0.5), [1.0, 0.0, 0.0, 0.5]);
    }
    #[test]
    fn hex_validation_and_alpha() {
        assert_eq!(parse_hex(" #f73e63 ", 0.25).unwrap()[3], 0.25);
        for invalid in ["FFF", "12345678", "GG0000", "é0000", "#12345"] {
            assert!(parse_hex(invalid, 1.0).is_none());
        }
        assert_eq!(hex([1.0, 0.0, 0.5, 0.5], true), "FF008080");
    }
    #[test]
    fn palette_roundtrip_and_corrupt_entries() {
        let colors = vec![[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 1.0]];
        assert_eq!(decode_palette(&encode_palette(&colors)), colors);
        assert_eq!(
            decode_palette("bad,FF000000,FF000000,💚💚"),
            vec![colors[0]]
        );
    }
}
