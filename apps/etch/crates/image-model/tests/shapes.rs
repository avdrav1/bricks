//! ETCH-11: lines, rectangles, and ellipses: their pixels at widths 1 and 5, outlined,
//! filled, or both, clipped at the image's edges, as rectangles for the preview, and one
//! undo step each.

use image_model::{Color, Document, Image, Kind, Pencil, Rect, Shape, ShapeTool, Style, TILE};

const RED: Color = Color::rgb(237, 28, 36);

fn tool(kind: Kind, width: u32) -> ShapeTool {
    ShapeTool {
        kind,
        width,
        outline: Color::BLACK,
        fill: RED,
    }
}

fn shape(kind: Kind, width: u32, from: (i64, i64), to: (i64, i64)) -> Shape {
    Shape {
        tool: tool(kind, width),
        from,
        to,
    }
}

/// The image as rows of `#` (black: the outline), `o` (red: the fill), and `.` (white).
fn mask(image: &Image) -> Vec<String> {
    (0..image.height())
        .map(|y| {
            (0..image.width())
                .map(|x| match image.pixel(x, y).unwrap() {
                    Color::BLACK => '#',
                    RED => 'o',
                    Color::WHITE => '.',
                    c => panic!("unexpected colour {c:?} at {x},{y}"),
                })
                .collect()
        })
        .collect()
}

/// Draws `shape` on a white `w`×`h` image; checks the changed rect holds every pixel it
/// changed; returns the mask.
fn drawn(w: u32, h: u32, shape: Shape) -> Vec<String> {
    let mut doc = Document::new(Image::new(w, h, Color::WHITE).unwrap());
    let changed = doc.shape(shape).expect("the shape is on the image");
    for (y, row) in mask(doc.image()).iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c != '.' {
                assert!(
                    changed.contains(x as u32, y as u32),
                    "{x},{y} outside {changed:?}"
                );
            }
        }
    }
    mask(doc.image())
}

#[test]
fn a_line_one_pixel_wide_is_bresenhams() {
    assert_eq!(
        drawn(12, 7, shape(Kind::Line, 1, (1, 1), (10, 5))),
        [
            "............",
            ".##.........",
            "...##.......",
            ".....##.....",
            ".......##...",
            ".........##.",
            "............",
        ]
    );
}

#[test]
fn a_line_five_pixels_wide_is_the_pencils_discs_along_it() {
    let want = [
        "..............",
        "..####........",
        ".#######......",
        ".#########....",
        ".###########..",
        "..###########.",
        "....#########.",
        "......#######.",
        "........####..",
        "..............",
    ];
    assert_eq!(drawn(14, 10, shape(Kind::Line, 5, (3, 3), (10, 6))), want);
    // The pencil's line, pixel for pixel.
    let mut doc = Document::new(Image::new(14, 10, Color::WHITE).unwrap());
    let pencil = Pencil {
        color: Color::BLACK,
        width: 5,
    };
    doc.begin_stroke(pencil, (3.5, 3.5));
    doc.extend_stroke((10.5, 6.5));
    doc.end_stroke();
    assert_eq!(mask(doc.image()), want);
}

#[test]
fn a_rectangle_outline_one_pixel_wide_runs_round_the_box() {
    let want = [
        "..........",
        ".########.",
        ".#......#.",
        ".#......#.",
        ".#......#.",
        ".#......#.",
        ".########.",
        "..........",
    ];
    let outline = Kind::Rectangle(Style::Outline);
    assert_eq!(drawn(10, 8, shape(outline, 1, (1, 1), (8, 6))), want);
    // Dragged the other way, the same box.
    assert_eq!(drawn(10, 8, shape(outline, 1, (8, 1), (1, 6))), want);
    assert_eq!(drawn(10, 8, shape(outline, 1, (8, 6), (1, 1))), want);
}

#[test]
fn a_rectangle_outline_five_pixels_wide_lies_inside_the_box() {
    let outline = [
        "................",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".#####....#####.",
        ".#####....#####.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        "................",
    ];
    let both = [
        "................",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".#####oooo#####.",
        ".#####oooo#####.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        ".##############.",
        "................",
    ];
    assert_eq!(
        drawn(
            16,
            14,
            shape(Kind::Rectangle(Style::Outline), 5, (1, 1), (14, 12))
        ),
        outline
    );
    assert_eq!(
        drawn(
            16,
            14,
            shape(Kind::Rectangle(Style::Both), 5, (1, 1), (14, 12))
        ),
        both
    );
    // Filled: the whole box the fill colour, whatever the width.
    let mut filled = vec!["................"; 14];
    filled[1..13].fill(".oooooooooooooo.");
    for width in [1, 5] {
        assert_eq!(
            drawn(
                16,
                14,
                shape(Kind::Rectangle(Style::Filled), width, (1, 1), (14, 12))
            ),
            filled
        );
    }
    // A box no wider than twice the outline is all outline.
    assert_eq!(
        drawn(
            16,
            14,
            shape(Kind::Rectangle(Style::Both), 5, (1, 1), (14, 9))
        )
        .iter()
        .filter(|r| r.contains('o'))
        .count(),
        0
    );
}

#[test]
fn an_ellipse_outline_one_pixel_wide_touches_the_box_sides() {
    let outline = Kind::Ellipse(Style::Outline);
    // Odd sides: a centre pixel.
    assert_eq!(
        drawn(13, 9, shape(outline, 1, (1, 1), (11, 7))),
        [
            ".............",
            "....#####....",
            "..##.....##..",
            ".#.........#.",
            ".#.........#.",
            ".#.........#.",
            "..##.....##..",
            "....#####....",
            ".............",
        ]
    );
    // Even sides: the centre between pixels, still symmetric.
    assert_eq!(
        drawn(12, 8, shape(outline, 1, (1, 1), (10, 6))),
        [
            "............",
            "...######...",
            "..#......#..",
            ".#........#.",
            ".#........#.",
            "..#......#..",
            "...######...",
            "............",
        ]
    );
    // A circle, 20 px across, joined through edges or corners all round.
    assert_eq!(
        drawn(22, 22, shape(outline, 1, (1, 1), (20, 20))),
        [
            "......................",
            "........######........",
            "......##......##......",
            ".....#..........#.....",
            "....#............#....",
            "...#..............#...",
            "..#................#..",
            "..#................#..",
            ".#..................#.",
            ".#..................#.",
            ".#..................#.",
            ".#..................#.",
            ".#..................#.",
            ".#..................#.",
            "..#................#..",
            "..#................#..",
            "...#..............#...",
            "....#............#....",
            ".....#..........#.....",
            "......##......##......",
            "........######........",
            "......................",
        ]
    );
    // Flat ones: a box two pixels wide, and one a pixel high.
    assert_eq!(
        drawn(6, 9, shape(outline, 1, (1, 1), (2, 7))),
        [
            "......", ".##...", ".##...", ".##...", ".##...", ".##...", ".##...", ".##...",
            "......"
        ]
    );
    assert_eq!(
        drawn(9, 3, shape(outline, 1, (1, 1), (7, 1))),
        [".........", ".#######.", "........."]
    );
}

#[test]
fn an_ellipse_outline_five_pixels_wide_lies_inside_the_box() {
    let outline = [
        ".......................",
        ".......#########.......",
        ".....#############.....",
        "....###############....",
        "...#################...",
        "..###################..",
        ".########.....########.",
        ".######.........######.",
        ".#####...........#####.",
        ".######.........######.",
        ".########.....########.",
        "..###################..",
        "...#################...",
        "....###############....",
        ".....#############.....",
        ".......#########.......",
        ".......................",
    ];
    assert_eq!(
        drawn(
            23,
            17,
            shape(Kind::Ellipse(Style::Outline), 5, (1, 1), (21, 15))
        ),
        outline
    );
    // Both: the inside, between the outline's sides, the fill colour.
    assert_eq!(
        drawn(
            23,
            17,
            shape(Kind::Ellipse(Style::Both), 5, (1, 1), (21, 15))
        ),
        [
            ".......................",
            ".......#########.......",
            ".....#############.....",
            "....###############....",
            "...#################...",
            "..###################..",
            ".########ooooo########.",
            ".######ooooooooo######.",
            ".#####ooooooooooo#####.",
            ".######ooooooooo######.",
            ".########ooooo########.",
            "..###################..",
            "...#################...",
            "....###############....",
            ".....#############.....",
            ".......#########.......",
            ".......................",
        ]
    );
    // A box narrower than the outline is all outline, and stays in the box.
    assert_eq!(
        drawn(
            9,
            6,
            shape(Kind::Ellipse(Style::Outline), 5, (1, 1), (3, 4))
        ),
        [
            ".........",
            ".###.....",
            ".###.....",
            ".###.....",
            ".###.....",
            "........."
        ]
    );
}

#[test]
fn a_filled_ellipse_is_the_one_pixel_outline_and_all_inside_it() {
    let want = [
        ".............",
        "....ooooo....",
        "..ooooooooo..",
        ".ooooooooooo.",
        ".ooooooooooo.",
        ".ooooooooooo.",
        "..ooooooooo..",
        "....ooooo....",
        ".............",
    ];
    for width in [1, 5] {
        assert_eq!(
            drawn(
                13,
                9,
                shape(Kind::Ellipse(Style::Filled), width, (1, 1), (11, 7))
            ),
            want
        );
    }
    assert_eq!(
        drawn(13, 9, shape(Kind::Ellipse(Style::Both), 1, (1, 1), (11, 7))),
        [
            ".............",
            "....#####....",
            "..##ooooo##..",
            ".#ooooooooo#.",
            ".#ooooooooo#.",
            ".#ooooooooo#.",
            "..##ooooo##..",
            "....#####....",
            ".............",
        ]
    );
}

#[test]
fn shapes_clip_at_the_image_edges() {
    // An ellipse partly off the top left: what's on the image is the same shape's pixels
    // there drawn on a larger image.
    assert_eq!(
        drawn(
            8,
            6,
            shape(Kind::Ellipse(Style::Outline), 1, (-4, -3), (5, 4))
        ),
        [".....#..", ".....#..", ".....#..", "...##...", "###.....", "........"]
    );
    let kinds = [
        Kind::Line,
        Kind::Rectangle(Style::Outline),
        Kind::Rectangle(Style::Both),
        Kind::Ellipse(Style::Outline),
        Kind::Ellipse(Style::Both),
        Kind::Ellipse(Style::Filled),
    ];
    for kind in kinds {
        for width in [1, 5] {
            for (from, to) in [((-6, -4), (9, 11)), ((3, 2), (25, 17)), ((-30, 5), (40, 9))] {
                let small = drawn(16, 12, shape(kind, width, from, to));
                let off = |(x, y): (i64, i64)| (x + 40, y + 40);
                let large = drawn(100, 100, shape(kind, width, off(from), off(to)));
                let crop: Vec<String> = large[40..52]
                    .iter()
                    .map(|r| r[40..56].to_string())
                    .collect();
                assert_eq!(small, crop, "{kind:?} {width} {from:?} {to:?}");
            }
        }
    }
    // Wholly off the image: nothing, and no step.
    let mut doc = Document::new(Image::new(16, 12, Color::WHITE).unwrap());
    assert_eq!(
        doc.shape(shape(Kind::Ellipse(Style::Both), 5, (20, 2), (30, 9))),
        None
    );
    assert_eq!(doc.shape(shape(Kind::Line, 1, (-9, -1), (-2, -8))), None);
    assert!(!doc.can_undo());
}

#[test]
fn a_shape_is_a_few_rectangles_that_paint_what_lands() {
    let bounds = Rect::new(0, 0, 40, 30);
    // Rows alike merge: a rectangle's outline is its top, its two sides, and its bottom.
    let r = shape(Kind::Rectangle(Style::Outline), 2, (5, 4), (30, 20));
    let rects = r.rects(bounds);
    assert_eq!(
        rects,
        [
            (Rect::new(5, 4, 26, 2), Color::BLACK),
            (Rect::new(5, 6, 2, 13), Color::BLACK),
            (Rect::new(29, 6, 2, 13), Color::BLACK),
            (Rect::new(5, 19, 26, 2), Color::BLACK),
        ]
    );
    assert_eq!(
        shape(Kind::Rectangle(Style::Both), 2, (5, 4), (30, 20))
            .rects(bounds)
            .len(),
        5
    );
    // Clipped to what's asked for.
    assert_eq!(
        r.rects(Rect::new(0, 0, 20, 5)),
        [(Rect::new(5, 4, 15, 1), Color::BLACK)]
    );
    // Painting the rectangles is the shape: none overlaps, every pixel is in one.
    for kind in [
        Kind::Line,
        Kind::Ellipse(Style::Both),
        Kind::Rectangle(Style::Both),
    ] {
        let s = shape(kind, 5, (3, 2), (36, 25));
        let mut doc = Document::new(Image::new(40, 30, Color::WHITE).unwrap());
        doc.shape(s);
        let mut seen = vec![None; 40 * 30];
        for (r, c) in s.rects(bounds) {
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    let k = (y * 40 + x) as usize;
                    assert_eq!(seen[k], None, "{kind:?}: {x},{y} twice");
                    seen[k] = Some(c);
                }
            }
        }
        for y in 0..30 {
            for x in 0..40 {
                let want = seen[(y * 40 + x) as usize].unwrap_or(Color::WHITE);
                assert_eq!(doc.image().pixel(x, y), Some(want), "{kind:?}: {x},{y}");
            }
        }
    }
}

#[test]
fn a_see_through_colour_lays_one_coat_over_whats_there() {
    let mut doc = Document::new(Image::new(12, 10, Color::WHITE).unwrap());
    let tool = ShapeTool {
        kind: Kind::Ellipse(Style::Both),
        width: 3,
        outline: Color::rgba(0, 0, 0, 128),
        fill: Color::rgba(0, 0, 255, 128),
    };
    doc.shape(Shape {
        tool,
        from: (0, 0),
        to: (11, 9),
    });
    // Half-transparent black over white, once (the disc's stamps overlap; the coat doesn't).
    assert_eq!(
        doc.image().pixel(0, 5),
        Some(Color::rgba(127, 127, 127, 255))
    );
    assert_eq!(
        doc.image().pixel(1, 5),
        Some(Color::rgba(127, 127, 127, 255))
    );
    assert_eq!(
        doc.image().pixel(6, 5),
        Some(Color::rgba(127, 127, 255, 255))
    );
    assert_eq!(doc.image().pixel(0, 0), Some(Color::WHITE));
}

#[test]
fn shift_keeps_lines_to_45_degrees_and_boxes_square() {
    let at = |kind, to: (f64, f64)| tool(kind, 1).at((10.5, 10.5), to, true).to;
    let line = Kind::Line;
    assert_eq!(at(line, (30.0, 14.0)), (30, 10));
    assert_eq!(at(line, (13.0, -20.0)), (10, -20));
    assert_eq!(at(line, (30.0, 25.0)), (30, 30));
    assert_eq!(at(line, (-5.0, 22.0)), (-5, 25));
    let ellipse = Kind::Ellipse(Style::Outline);
    assert_eq!(at(ellipse, (30.0, 14.0)), (30, 30));
    assert_eq!(at(ellipse, (4.0, -20.0)), (-20, -20));
    assert_eq!(at(Kind::Rectangle(Style::Filled), (12.0, 3.0)), (17, 3));
    // Without Shift, where the pointer is; fractions floor.
    assert_eq!(
        tool(line, 1).at((10.5, 10.5), (30.9, 14.2), false),
        Shape {
            tool: tool(line, 1),
            from: (10, 10),
            to: (30, 14)
        }
    );
}

#[test]
fn each_shape_undoes_and_redoes_byte_for_byte() {
    let mut doc = Document::new(Image::new(300, 200, Color::rgba(10, 20, 30, 128)).unwrap());
    let blank = doc.image().clone();
    let shapes = [
        shape(Kind::Line, 5, (-10, 7), (290, 180)),
        shape(Kind::Rectangle(Style::Both), 4, (20, 30), (250, 150)),
        shape(Kind::Ellipse(Style::Outline), 1, (60, -40), (280, 190)),
        shape(Kind::Ellipse(Style::Filled), 5, (100, 50), (130, 90)),
    ];
    let mut after = vec![blank.clone()];
    for s in shapes {
        doc.shape(s).unwrap();
        after.push(doc.image().clone());
    }
    for i in (0..shapes.len()).rev() {
        doc.undo().unwrap();
        assert_eq!(doc.image(), &after[i], "undo to {i}");
    }
    assert!(!doc.can_undo());
    for (i, want) in after.iter().enumerate().skip(1) {
        doc.redo().unwrap();
        assert_eq!(doc.image(), want, "redo to {i}");
    }
    assert!(!doc.can_redo());
}

#[test]
fn a_shape_keeps_only_the_tiles_it_touched() {
    let tile = (TILE * TILE * 4) as usize;
    // An outline round a 1024-px square touches the 60 tiles along its edge, not the 196
    // inside; filled, all 256.
    let mut doc = Document::new(Image::new(1024, 1024, Color::WHITE).unwrap());
    let changed = doc.shape(shape(
        Kind::Rectangle(Style::Outline),
        1,
        (10, 10),
        (1000, 1000),
    ));
    assert_eq!(changed, Some(Rect::new(10, 10, 991, 991)));
    assert_eq!(doc.history_bytes(), 60 * tile);
    doc.shape(shape(
        Kind::Ellipse(Style::Outline),
        3,
        (10, 10),
        (1000, 1000),
    ));
    let ring = doc.history_bytes() - 60 * tile;
    assert!(ring < 100 * tile, "the ring keeps {} tiles", ring / tile);
    doc.shape(shape(
        Kind::Rectangle(Style::Filled),
        1,
        (10, 10),
        (1000, 1000),
    ));
    assert_eq!(doc.history_bytes() - 60 * tile - ring, 256 * tile);
    // Its step is one: one undo takes all of it.
    doc.undo();
    doc.undo();
    doc.undo();
    assert_eq!(
        doc.image(),
        Image::new(1024, 1024, Color::WHITE).as_ref().unwrap()
    );
}
