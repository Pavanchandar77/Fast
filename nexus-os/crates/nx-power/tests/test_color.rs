use nx_power::color::{Color, ColorHistoryStore};

#[test]
fn test_color_from_hex() {
    // 3 digits
    let c1 = Color::from_hex("#F0A").unwrap();
    assert_eq!(c1, Color::rgb(0xFF, 0x00, 0xAA));

    // 4 digits (with alpha)
    let c2 = Color::from_hex("#F0AF").unwrap();
    assert_eq!(c2, Color::rgba(0xFF, 0x00, 0xAA, 0xFF));

    // 6 digits
    let c3 = Color::from_hex("#336699").unwrap();
    assert_eq!(c3, Color::rgb(0x33, 0x66, 0x99));

    // 8 digits
    let c4 = Color::from_hex("#336699CC").unwrap();
    assert_eq!(c4, Color::rgba(0x33, 0x66, 0x99, 0xCC));

    // Without leading '#'
    let c5 = Color::from_hex("FFFFFF").unwrap();
    assert_eq!(c5, Color::WHITE);

    // Invalid length
    assert!(Color::from_hex("#12345").is_err());
    // Invalid characters
    assert!(Color::from_hex("#ZZZZZZ").is_err());
}

#[test]
fn test_color_hex_formatting() {
    let color = Color::rgba(255, 128, 0, 255);
    assert_eq!(color.to_hex_rgb(), "#FF8000");
    assert_eq!(color.to_hex_rgba(), "#FF8000FF");

    // Short hex where nibbles match
    let short_eligible = Color::rgb(0x11, 0x22, 0x33);
    assert_eq!(short_eligible.to_hex_short(), Some("#123".to_string()));

    let not_short = Color::rgb(0x12, 0x34, 0x56);
    assert_eq!(not_short.to_hex_short(), None);
}

#[test]
fn test_color_css_strings() {
    let color = Color::rgba(255, 0, 128, 204); // alpha ~ 0.80
    assert_eq!(color.to_rgb_css(), "rgb(255, 0, 128)");
    assert_eq!(color.to_rgba_css(), "rgba(255, 0, 128, 0.80)");
}

#[test]
fn test_color_hsl_conversions() {
    // Pure Red
    let red = Color::rgb(255, 0, 0);
    let (h, s, l) = red.to_hsl();
    assert!((h - 0.0).abs() < 1.0);
    assert!((s - 1.0).abs() < 0.01);
    assert!((l - 0.5).abs() < 0.01);
    assert_eq!(red.to_hsl_css(), "hsl(0, 100%, 50%)");

    // Reconstruct from HSL
    let recon = Color::from_hsl(h, s, l);
    assert_eq!(recon, red);

    // Pure Green
    let green = Color::rgb(0, 255, 0);
    let (h_g, _, _) = green.to_hsl();
    assert!((h_g - 120.0).abs() < 1.0);

    // Pure Blue
    let blue = Color::rgb(0, 0, 255);
    let (h_b, _, _) = blue.to_hsl();
    assert!((h_b - 240.0).abs() < 1.0);
}

#[test]
fn test_color_hsv_conversions() {
    let color = Color::from_hsv(180.0, 1.0, 1.0); // Cyan
    assert_eq!(color, Color::rgb(0, 255, 255));

    let (h, s, v) = color.to_hsv();
    assert!((h - 180.0).abs() < 1.0);
    assert!((s - 1.0).abs() < 0.01);
    assert!((v - 1.0).abs() < 0.01);
}

#[test]
fn test_color_cmyk_conversions() {
    let black = Color::BLACK;
    let (c, m, y, k) = black.to_cmyk();
    assert_eq!((c, m, y, k), (0.0, 0.0, 0.0, 1.0));
    assert_eq!(black.to_cmyk_css(), "cmyk(0%, 0%, 0%, 100%)");

    let white = Color::WHITE;
    let (c_w, m_w, y_w, k_w) = white.to_cmyk();
    assert_eq!((c_w, m_w, y_w, k_w), (0.0, 0.0, 0.0, 0.0));
}

#[test]
fn test_wcag_luminance_and_contrast() {
    let white = Color::WHITE;
    let black = Color::BLACK;

    assert!((white.relative_luminance() - 1.0).abs() < 0.001);
    assert!((black.relative_luminance() - 0.0).abs() < 0.001);

    // Contrast between black and white is exactly 21:1
    let contrast = white.contrast_ratio(&black);
    assert!((contrast - 21.0).abs() < 0.1);

    // Self-contrast is 1:1
    assert!((white.contrast_ratio(&white) - 1.0).abs() < 0.001);
}

#[test]
fn test_color_history_and_palettes() {
    let mut store = ColorHistoryStore::new(3);

    let c1 = Color::rgb(255, 0, 0);
    let c2 = Color::rgb(0, 255, 0);
    let c3 = Color::rgb(0, 0, 255);
    let c4 = Color::rgb(255, 255, 0);

    store.push(c1);
    store.push(c2);
    store.push(c3);
    assert_eq!(store.recent().len(), 3);

    // Pushing c4 evicts oldest (c1)
    store.push(c4);
    assert_eq!(store.recent().len(), 3);
    assert_eq!(store.recent()[0], c4);
    assert!(!store.recent().contains(&c1));

    // Deduplication moves existing to front
    store.push(c2);
    assert_eq!(store.recent()[0], c2);

    // Palettes
    store.create_palette("Primary", vec![c1, c2, c3]);
    let pal = store.get_palette("Primary").unwrap();
    assert_eq!(pal.len(), 3);

    // Favorites
    store.add_favorite(c1);
    assert_eq!(store.favorites(), &[c1]);
    store.remove_favorite(&c1);
    assert!(store.favorites().is_empty());
}
