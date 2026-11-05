// SPDX-FileCopyrightText: 2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Unit tests for icon resolution and pixbuf-to-cairo conversion: command
//! tokenising, desktop-entry icon kinds, premultiplication and the negative
//! paths (blank commands, unknown icons, empty caches).

use gtk4::{
    gdk_pixbuf::{Colorspace, Pixbuf},
    gio
};

use super::{IconCache, IconSource, exec_key, icon_source, surface_from_pixbuf};

#[test]
fn exec_key_takes_the_basename_of_the_first_token() {
    assert_eq!(
        exec_key("/usr/bin/thunar --daemon").as_deref(),
        Some("thunar")
    );
    assert_eq!(
        exec_key("  alacritty   -e top").as_deref(),
        Some("alacritty")
    );
    assert_eq!(exec_key("firefox/").as_deref(), Some("firefox"));
    assert_eq!(
        exec_key("\"/opt/my app/bin/app\" %U").as_deref(),
        Some("app")
    );
}

#[test]
fn exec_key_rejects_blank_and_nameless_commands() {
    assert_eq!(exec_key(""), None);
    assert_eq!(exec_key("   "), None);
    assert_eq!(exec_key("/"), None);
    assert_eq!(exec_key(" / "), None);
    assert_eq!(exec_key("\"unclosed arg"), None);
}

#[test]
fn icon_source_reads_themed_names_first() {
    let icon: gio::Icon = gio::ThemedIcon::from_names(&["one", "two"]).into();
    assert_eq!(
        icon_source(Some(&icon)),
        Some(IconSource::Named("one".to_string()))
    );
}

#[test]
fn icon_source_reads_file_icons_as_paths() {
    let file = gio::File::for_path("/usr/share/pixmaps/foo.png");
    let icon: gio::Icon = gio::FileIcon::new(&file).into();
    assert_eq!(
        icon_source(Some(&icon)),
        Some(IconSource::File("/usr/share/pixmaps/foo.png".into()))
    );
}

#[test]
fn icon_source_rejects_missing_icons() {
    assert_eq!(icon_source(None), None);
}

#[test]
fn surface_converts_rgb_pixels_to_premultiplied_words() {
    let pixbuf = Pixbuf::new(Colorspace::Rgb, true, 8, 2, 1).expect("a 2x1 pixbuf");
    pixbuf.put_pixel(0, 0, 10, 20, 30, 255);
    pixbuf.put_pixel(1, 0, 128, 128, 128, 127);
    let mut surface = surface_from_pixbuf(&pixbuf).expect("a 2x1 image converts");
    assert_eq!((surface.width(), surface.height()), (2, 1));
    let data = surface.data().expect("image data");
    let first = u32::from_ne_bytes(data[0..4].try_into().expect("one word"));
    assert_eq!(first, 0xFF0A_141E);
    let second = u32::from_ne_bytes(data[4..8].try_into().expect("one word"));
    assert_eq!(second, 0x7F3F_3F3F, "colour is premultiplied by alpha");
}

#[test]
fn surface_fully_transparent_pixel_is_zero() {
    let pixbuf = Pixbuf::new(Colorspace::Rgb, true, 8, 1, 1).expect("a 1x1 pixbuf");
    pixbuf.put_pixel(0, 0, 200, 100, 50, 0);
    let mut surface = surface_from_pixbuf(&pixbuf).expect("a 1x1 image converts");
    let data = surface.data().expect("image data");
    let word = u32::from_ne_bytes(data[0..4].try_into().expect("one word"));
    assert_eq!(word, 0, "premultiplied transparent is black-transparent");
}

#[test]
fn surface_treats_rgb_pixbufs_as_opaque() {
    let pixbuf = Pixbuf::new(Colorspace::Rgb, false, 8, 1, 1).expect("a 1x1 pixbuf");
    pixbuf.put_pixel(0, 0, 0x12, 0x34, 0x56, 0);
    let mut surface = surface_from_pixbuf(&pixbuf).expect("a 1x1 image converts");
    let data = surface.data().expect("image data");
    let word = u32::from_ne_bytes(data[0..4].try_into().expect("one word"));
    assert_eq!(word, 0xFF12_3456, "no alpha channel means fully opaque");
}

#[test]
fn cache_surface_is_none_before_warming() {
    let cache = IconCache::new();
    assert!(cache.surface(Some("thunar"), 68).is_none());
    assert!(cache.surface(None, 68).is_none());
}
