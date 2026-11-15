//! Looking at the model through the session: the `render` and `view` queries, and the
//! PNG file read back by a reader that shares no code with the writer.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::Accuracy;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::{MethodKind, Reply, Session};
use krly3d_view::png;
use std::f64::consts::PI;
use std::path::Path;
use std::time::Instant;

fn session() -> Session {
    let mut builder = krly3d_std::standard();
    krly3d_view::register(&mut builder);
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    builder.session()
}

/// A session holding a plate of 60 by 40 by 10 and a ball of radius 5.
fn workshop() -> Session {
    let mut session = session();
    let plate = Params::new().with("id", "plate").with("w", 60.0).with("d", 40.0).with("h", 10.0);
    let ball = Params::new().with("id", "ball").with("r", 5.0).with("at", [0.0, -60.0, 5.0]);
    assert!(session.call("box", &plate).ok && session.call("sphere", &ball).ok);
    session
}

fn entry<'a>(value: &'a Value, name: &str) -> &'a Value {
    let Value::Map(entries) = value else { panic!("not a map: {value:?}") };
    entries.get(name).unwrap_or_else(|| panic!("no `{name}` in {value:?}"))
}

fn number(value: &Value, name: &str) -> f64 {
    let Value::Number(number) = entry(value, name) else { panic!("`{name}` is not a number") };
    *number
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    let Value::Text(text) = entry(value, name) else { panic!("`{name}` is not a text") };
    text
}

/// The faces a `view` reply lists, by name, with the number of pixels that show each.
fn seen(reply: &Reply) -> Vec<(String, f64)> {
    assert!(reply.ok, "{}", reply.text);
    let Value::List(faces) = entry(&reply.data, "faces") else { panic!("faces are not a list") };
    faces.iter().map(|face| (text(face, "name").to_string(), number(face, "pixels"))).collect()
}

fn names(reply: &Reply) -> Vec<String> {
    let mut names: Vec<String> = seen(reply).into_iter().map(|(name, _)| name).collect();
    names.sort();
    names
}

/// The picture of a `render` reply, read back.
fn picture(reply: &Reply) -> Picture {
    assert!(reply.ok, "{}", reply.text);
    read_png(&bytes_of_base64(text(&reply.data, "png")))
}

struct Picture {
    width: usize,
    height: usize,
    channels: usize,
    pixels: Vec<u8>,
}

/// Reads a PNG file whose pixels are stored uncompressed, checking on the way everything
/// a reader may rely on: the signature, the order and the checksums of the chunks, the
/// header, the framing of the stored blocks and the checksum of the pixel data.
fn read_png(file: &[u8]) -> Picture {
    assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n", "the signature");
    let mut chunks: Vec<(&[u8], &[u8])> = Vec::new();
    let mut at = 8;
    while at < file.len() {
        let length = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        let (kind, data) = (&file[at + 4..at + 8], &file[at + 8..at + 8 + length]);
        let stated = u32::from_be_bytes(file[at + 8 + length..at + 12 + length].try_into().unwrap());
        assert_eq!(stated, crc_bit_by_bit(&file[at + 4..at + 8 + length]), "the checksum of a chunk");
        chunks.push((kind, data));
        at += 12 + length;
    }
    let kinds: Vec<&[u8]> = chunks.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(kinds, [b"IHDR".as_slice(), b"IDAT", b"IEND"], "the chunks and their order");
    let (header, stream) = (chunks[0].1, chunks[1].1);
    assert_eq!((header.len(), chunks[2].1.len()), (13, 0));
    let width = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
    let height = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
    let channels = match header[9] {
        2 => 3,
        6 => 4,
        other => panic!("colour type {other}"),
    };
    assert_eq!((header[8], header[10], header[11], header[12]), (8, 0, 0, 0), "depth, methods, interlace");

    assert_eq!(stream[0] & 0x0f, 8, "the deflate method");
    assert_eq!((u32::from(stream[0]) * 256 + u32::from(stream[1])) % 31, 0, "the check bits of the stream");
    assert_eq!(stream[1] & 0x20, 0, "no preset dictionary");
    let (mut at, mut lines) = (2, Vec::new());
    loop {
        let (last, length) = (stream[at], u16::from_le_bytes([stream[at + 1], stream[at + 2]]));
        assert!(last <= 1, "a stored block, marked last or not");
        assert_eq!(u16::from_le_bytes([stream[at + 3], stream[at + 4]]), !length, "the length, inverted");
        lines.extend_from_slice(&stream[at + 5..at + 5 + usize::from(length)]);
        at += 5 + usize::from(length);
        if last == 1 {
            break;
        }
    }
    let stated = u32::from_be_bytes(stream[at..at + 4].try_into().unwrap());
    assert_eq!(stated, adler_by_definition(&lines), "the checksum of the pixel data");
    assert_eq!(at + 4, stream.len(), "nothing after the checksum");

    let line = 1 + width * channels;
    assert_eq!(lines.len(), height * line, "one filter byte and a row of pixels per line");
    let mut pixels = Vec::with_capacity(width * height * channels);
    for row in lines.chunks(line) {
        assert_eq!(row[0], 0, "the filter of a line");
        pixels.extend_from_slice(&row[1..]);
    }
    Picture { width, height, channels, pixels }
}

/// CRC-32 one bit at a time, straight from its polynomial.
fn crc_bit_by_bit(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Adler-32 as it is defined: both sums reduced after every byte.
fn adler_by_definition(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1_u32, 0_u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    high << 16 | low
}

fn bytes_of_base64(text: &str) -> Vec<u8> {
    let digits = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    for group in text.as_bytes().chunks(4) {
        assert_eq!(group.len(), 4, "base64 comes in groups of four");
        let padding = group.iter().rev().take_while(|digit| **digit == b'=').count();
        let mut word = 0_u32;
        for digit in &group[..4 - padding] {
            word = word << 6 | digits.find(char::from(*digit)).unwrap() as u32;
        }
        word <<= 6 * padding;
        bytes.extend_from_slice(&word.to_be_bytes()[1..4 - padding]);
    }
    bytes
}

#[test]
fn the_two_queries_are_methods_of_the_session() {
    let session = session();

    let methods = session.methods();

    for (name, expected) in [
        ("render", &["nodes", "view", "width", "height", "renderer", "paint"][..]),
        ("view", &["nodes", "view", "width", "height", "renderer"][..]),
    ] {
        let method = methods.iter().find(|method| method.name == name).unwrap();
        let arguments: Vec<&str> = method.schema.params.iter().map(|param| param.name).collect();
        assert_eq!(method.kind, MethodKind::Query);
        assert_eq!(arguments, expected);
        assert!(method.schema.params.iter().all(|param| !param.required), "`{name}` needs an argument");
    }
}

#[test]
fn a_box_seen_from_the_front_shows_only_its_front_over_the_share_of_the_image_it_covers() {
    let mut session = workshop();

    let reply = session.call("view", &Params::new().with("nodes", "plate").with("view", "front"));

    // Fitted to 90% of the 480 pixels across, the front of 60 by 10 covers 432 by 72 pixels.
    let seen = seen(&reply);
    assert_eq!(seen.len(), 1, "{}", reply.text);
    assert_eq!(seen[0].0, "plate#front");
    assert!((seen[0].1 - 432.0 * 72.0).abs() <= 432.0 + 72.0, "{} pixels", seen[0].1);
    assert_eq!((number(&reply.data, "width"), number(&reply.data, "height")), (480.0, 360.0));
}

#[test]
fn a_box_seen_from_the_iso_view_shows_exactly_its_top_front_and_right() {
    let mut session = workshop();

    let reply = session.call("view", &Params::new().with("nodes", "plate"));

    assert_eq!(names(&reply), ["plate#front", "plate#right", "plate#top"]);
}

#[test]
fn every_named_side_view_shows_the_face_of_a_box_it_is_named_after() {
    let mut session = workshop();
    for side in ["front", "back", "left", "right", "top", "bottom"] {
        let look =
            Params::new().with("nodes", "plate").with("view", side).with("width", 120.0).with("height", 90.0);

        let reply = session.call("view", &look);

        assert_eq!(names(&reply), [format!("plate#{side}")]);
    }
}

#[test]
fn the_outline_of_a_ball_covers_the_area_of_its_great_circle() {
    let mut session = workshop();

    let reply = session.call("view", &Params::new().with("nodes", "ball"));

    let (pixel, seen) = (number(&reply.data, "pixel"), seen(&reply));
    let covered = seen[0].1 * pixel * pixel;
    let one_pixel_along_the_outline = 2.0 * PI * 5.0 * pixel;
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "ball#skin");
    assert!((covered - PI * 25.0).abs() < one_pixel_along_the_outline, "{covered} against {}", PI * 25.0);
}

#[test]
fn a_body_in_front_hides_what_is_behind_it() {
    let mut session = workshop();

    let reply = session.call("view", &Params::new().with("view", "front"));

    // The ball of radius 5 stands in front of the plate, which is 10 high: within the
    // outline of the ball nothing of the plate is seen, beside it the front is.
    let seen = seen(&reply);
    let pixel = number(&reply.data, "pixel");
    let pixels = |name: &str| seen.iter().find(|(seen, _)| seen == name).unwrap().1;
    let whole_front = 60.0 * 10.0 / (pixel * pixel);
    let ball = PI * 25.0 / (pixel * pixel);
    assert_eq!(names(&reply), ["ball#skin", "plate#front"]);
    assert!((pixels("ball#skin") - ball).abs() < 2.0 * PI * 5.0 / pixel);
    assert!(
        (pixels("plate#front") - (whole_front - ball)).abs() < 2.0 * PI * 5.0 / pixel + 2.0 * 70.0 / pixel
    );
}

#[test]
fn the_picture_is_a_png_file_a_reader_of_its_own_accepts() {
    let mut session = workshop();

    let reply = session.call("render", &Params::new());

    let picture = picture(&reply);
    assert_eq!((picture.width, picture.height, picture.channels), (480, 360, 3));
    assert_eq!(picture.pixels.len(), 480 * 360 * 3);
    assert_eq!(&picture.pixels[..3], [255, 255, 255], "the corner of a fitted view is background");
}

#[test]
fn the_picture_shows_as_many_pixels_of_the_model_as_the_view_counts() {
    let mut session = workshop();
    let look = Params::new().with("width", 200.0).with("height", 150.0);

    let (drawn, described) = (session.call("render", &look), session.call("view", &look));

    let picture = picture(&drawn);
    let painted = picture.pixels.chunks(3).filter(|pixel| *pixel != [255, 255, 255]).count();
    let counted: f64 = seen(&described).iter().map(|(_, pixels)| pixels).sum();
    assert_eq!(painted as f64, counted);
    assert_eq!(painted as f64 + number(entry(&described.data, "background"), "pixels"), 200.0 * 150.0);
}

#[test]
fn pixels_come_back_from_a_png_file_as_they_went_in() {
    // More than one stored block of 65535 bytes, with and without opacity.
    for (width, height, channels) in [(300, 100, 4), (211, 173, 3), (1, 1, 3)] {
        let pixels: Vec<u8> = (0..width * height * channels).map(|at| (at * 7 + at / 251) as u8).collect();

        let file = png::encode(width, height, channels, &pixels).unwrap();

        let picture = read_png(&file);
        assert_eq!((picture.width, picture.height, picture.channels), (width, height, channels));
        assert!(picture.pixels == pixels, "the pixels of a {width} by {height} image changed");
    }
}

#[test]
fn a_model_file_is_drawn_from_every_named_view_with_the_plate_seen_largest_from_its_side() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/pegboard.k3d");
    let mut session = session();
    let opened = session.call("open", &Params::new().with("text", std::fs::read_to_string(file).unwrap()));
    assert!(opened.ok, "{}", opened.text);
    let views = ["iso", "front", "back", "left", "right", "top", "bottom"];
    let largest = ["top", "front", "back", "left", "right", "top", "bottom"];
    for (view, face) in views.into_iter().zip(largest) {
        let look = Params::new().with("view", view).with("width", 96.0).with("height", 72.0);

        let reply = session.call("view", &look);

        assert_eq!(seen(&reply)[0].0, format!("plate#{face}"), "{view}: {}", reply.text);
    }
}

#[test]
fn where_a_node_coincides_with_what_it_was_made_from_the_later_one_is_seen() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/pegboard.k3d");
    let mut session = session();
    let opened = session.call("open", &Params::new().with("text", std::fs::read_to_string(file).unwrap()));
    assert!(opened.ok, "{}", opened.text);

    let reply = session.call("view", &Params::new().with("width", 96.0).with("height", 72.0));

    // `pegs` repeats `peg` three times, the first of them in the place of `peg` itself.
    let names = names(&reply);
    assert!(names.contains(&"pegs#side[1]".to_string()), "{}", reply.text);
    assert!(!names.iter().any(|name| name.starts_with("peg#")), "{}", reply.text);
}

#[test]
fn a_480_by_360_picture_of_a_ball_and_of_a_box_is_timed() {
    let mut session = workshop();
    for node in ["ball", "plate"] {
        let started = Instant::now();

        let reply = session.call("render", &Params::new().with("nodes", node));

        // Reported, not judged: how long a picture takes depends on the machine and the build.
        println!("render of `{node}` at 480x360: {:.1} ms", started.elapsed().as_secs_f64() * 1e3);
        assert!(reply.ok, "{}", reply.text);
    }
}
