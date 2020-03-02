use std::cmp;

#[allow(many_single_char_names)]
pub fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let max = cmp::max(cmp::max(r, g), b);
    let min = cmp::min(cmp::min(r, g), b);
    let c = max - min;

    let rr = f64::from(r);
    let gg = f64::from(g);
    let bb = f64::from(b);
    let cc = f64::from(c);
    let h = match max {
        _ if c == 0 => 0.0,
        max if max == r => ((gg - bb) / cc) % 6.0,
        max if max == g => ((bb - rr) / cc) + 2.0,
        max if max == b => ((rr - gg) / cc) + 4.0,
        _ => unreachable!(),
    } / 6.0;
    let h = (1.0 + h) % 1.0;

    let l = ((f64::from(max) + f64::from(min)) * 0.5) / 255.0;

    let s = if c == 0 {
        0.0
    } else {
        cc / (1.0 - (2.0 * l - 1.0).abs())
    } / 255.0;

    (h, s, l)
}

#[allow(many_single_char_names)]
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hh = h * 6.0;
    let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
    let rgb = match hh {
        hh if hh >= 0.0 && hh < 1.0 => (c, x, 0.0),
        hh if hh >= 1.0 && hh < 2.0 => (x, c, 0.0),
        hh if hh >= 2.0 && hh < 3.0 => (0.0, c, x),
        hh if hh >= 3.0 && hh < 4.0 => (0.0, x, c),
        hh if hh >= 4.0 && hh < 5.0 => (x, 0.0, c),
        hh if hh >= 5.0 && hh < 6.0 => (c, 0.0, x),
        _ => unreachable!(),
    };
    let m = l - c * 0.5;
    (
        ((m + rgb.0) * 255.0).round() as u8,
        ((m + rgb.1) * 255.0).round() as u8,
        ((m + rgb.2) * 255.0).round() as u8,
    )
}
