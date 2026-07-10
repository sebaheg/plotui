//! Encoder tests: escape-sequence structure, not terminal behavior.

use plotui_core::{Plot, PALETTE};
use plotui_protocol::{
    halfblock, kitty, kitty_cleanup, kitty_placeholder, kitty_placeholder_cells,
};

fn frame(w: usize, h: usize) -> plotui_core::Framebuffer {
    let mut p = Plot::new();
    p.add_line2d(vec![0.0, 1.0, 2.0], vec![0.0, 2.0, 1.0], PALETTE[0], 2.0, None);
    p.render(w, h)
}

#[test]
fn kitty_escape_is_wrapped_and_ids_the_image() {
    let s = kitty(&frame(80, 40), 20, 10);
    assert!(s.starts_with("\x1b[s\x1b_G"), "save cursor, then APC");
    assert!(s.ends_with("\x1b[u"), "restore cursor at the end");
    assert!(s.contains("q=2"), "responses suppressed");
    assert!(s.contains("i=4242"), "fixed image id for atomic replace");
    assert!(s.contains("s=80,v=40"), "source pixel size");
    assert!(s.contains("c=20,r=10"), "target cell region");
}

#[test]
fn kitty_frame_replaces_rather_than_stacks() {
    // a=T creates a new placement every time; without both of these, each
    // repaint piles another copy onto the screen (and they outlive the app).
    let s = kitty(&frame(80, 40), 20, 10);
    assert!(
        s.starts_with("\x1b[s\x1b_Ga=d,d=i,i=4242,q=2\x1b\\"),
        "previous placements are deleted before the new frame is placed"
    );
    assert!(s.contains("p=1,a=T"), "fixed placement id, for terminals that replace by p=");
    assert_eq!(s.matches("a=T").count(), 1, "exactly one placement per frame");
    assert_eq!(s.matches("a=d").count(), 1, "exactly one delete per frame");
}

#[test]
fn kitty_chunks_stay_within_the_4k_apc_limit() {
    // A large frame forces multiple chunks; every chunk must be terminated.
    let s = kitty(&frame(600, 400), 80, 25);
    let chunks = s.matches("\x1b_G").count();
    assert!(chunks > 1, "large payload should chunk");
    assert_eq!(s.matches("\x1b\\").count(), chunks, "every APC is terminated");
    assert_eq!(s.matches("m=0").count(), 1, "exactly one final chunk");
}

#[test]
fn placeholder_emits_one_row_per_cell_row() {
    let p = kitty_placeholder(&frame(80, 40), 20, 10);
    assert_eq!(p.rows.len(), 10);
    for row in &p.rows {
        assert_eq!(row.chars().filter(|c| *c == '\u{10EEEE}').count(), 20);
    }
    assert!(p.transmit.contains("U=1"), "virtual placement");
    assert!(p.transmit.contains("i=4242"));
    // Image id 4242 = 0x00001092 → encoded in the placeholder foreground color.
    assert_eq!(p.id_rgb, (0x00, 0x10, 0x92));
}

#[test]
fn placeholder_cells_are_individually_addressed() {
    let p = kitty_placeholder_cells(&frame(80, 40), 20, 10);
    assert_eq!(p.cells.len(), 10);
    assert!(p.transmit.contains("U=1"), "virtual placement");
    assert_eq!(p.id_rgb, (0x00, 0x10, 0x92), "same id encoding as the row variant");
    for (y, row) in p.cells.iter().enumerate() {
        assert_eq!(row.len(), 20);
        for cell in row {
            // PLACEHOLDER + row diacritic + column diacritic; the "extra" id
            // diacritic is omitted because 4242 fits in the color's 24 bits.
            let chars: Vec<char> = cell.chars().collect();
            assert_eq!(chars.len(), 3);
            assert_eq!(chars[0], '\u{10EEEE}');
        }
        // Same row → same row diacritic; distinct columns → distinct column
        // diacritics. This is what makes a row survive text spliced into it:
        // cells after the gap still say exactly where they belong.
        let row_marks: Vec<char> = row.iter().map(|c| c.chars().nth(1).unwrap()).collect();
        assert!(row_marks.iter().all(|m| *m == row_marks[0]));
        let mut col_marks: Vec<char> = row.iter().map(|c| c.chars().nth(2).unwrap()).collect();
        col_marks.dedup();
        assert_eq!(col_marks.len(), 20, "row {y}: column diacritics must be distinct");
    }
    // Distinct rows carry distinct row diacritics.
    let mut row_marks: Vec<char> = p.cells.iter().map(|r| r[0].chars().nth(1).unwrap()).collect();
    row_marks.dedup();
    assert_eq!(row_marks.len(), 10);
}

#[test]
fn halfblock_yields_one_line_per_cell_row() {
    let out = halfblock(&frame(40, 40)); // 40px tall → 20 text rows
    assert_eq!(out.lines().count(), 20);
    assert!(out.contains("\u{2580}") || out.contains("\u{2584}"), "some pixels rendered");
    for line in out.lines() {
        assert!(line.ends_with("\x1b[0m"), "attributes reset per line");
    }
}

#[test]
fn halfblock_of_an_empty_plot_is_blank_but_shaped() {
    let mut p = Plot::new();
    p.show_box = false; // otherwise the 3D orientation box still draws
    let out = halfblock(&p.render(10, 10));
    assert_eq!(out.lines().count(), 5);
    assert!(!out.contains('\u{2580}'));
}

#[test]
fn cleanup_deletes_exactly_our_image() {
    assert_eq!(kitty_cleanup(), "\x1b_Ga=d,d=i,i=4242\x1b\\");
}

#[test]
fn kitty_compat_repeats_the_id_on_every_chunk() {
    use plotui_protocol::kitty_compat;
    // Big frame → many chunks. iTerm2 drops spec-framed (bare m=) chunk
    // streams, so the compat variant must carry the id on every chunk.
    let s = kitty_compat(&frame(600, 400), 80, 25);
    let apcs: Vec<&str> = s.split("\x1b_G").skip(1).collect(); // [delete, chunk0, chunk1, ...]
    assert!(apcs.len() > 3, "expected a delete plus several data chunks");
    for (i, apc) in apcs.iter().enumerate() {
        assert!(apc.contains("i=4242"), "APC {i} must carry the image id");
    }
    // Same replace-not-stack contract as the spec-framed variant.
    assert!(s.starts_with("\x1b[s\x1b_Ga=d,d=i,i=4242,q=2\x1b\\"));
    assert_eq!(s.matches("a=T").count(), 1);
    assert_eq!(s.matches("m=0").count(), 1, "exactly one final chunk");
    // The spec-framed variant stays spec-framed (Kitty/Ghostty path):
    // APCs after [cursor-save prefix, delete, header chunk] are continuations.
    let spec = kitty(&frame(600, 400), 80, 25);
    let bare: Vec<&str> = spec.split("\x1b_G").skip(3).collect();
    assert!(!bare.is_empty(), "large frame must have continuation chunks");
    assert!(bare.iter().all(|c| c.starts_with("m=")), "spec continuations carry only m=");
}
