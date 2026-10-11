//! SHEET-1: adding, renaming, moving, and deleting sheets are undoable steps (ADR 0008).
//! Each undo and redo puts the sheet list and every sheet's edits exactly as they were,
//! and sheet names follow Excel's rules.

use commands::{Command, OnSheet, SetCell, SheetEdit, UndoStack};
use data_model::{BookEdit, CellRef, CsvTable, Origin, SheetId, Workbook};

/// A table of one row with `text` in A1.
fn table(text: &str) -> CsvTable {
    let mut t = CsvTable::untitled();
    let rows = t.insert_rows(0, 1).unwrap();
    t.apply(rows);
    let at = CellRef {
        row: t.row_id(0),
        col: t.col_id(0),
    };
    t.apply(data_model::Edit::set(at, text));
    t
}

type State = (Vec<(SheetId, String)>, SheetId, Vec<usize>);

fn state(book: &Workbook) -> State {
    (
        book.sheets()
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect(),
        book.active_id(),
        book.sheets().iter().map(|s| s.table.changes()).collect(),
    )
}

#[test]
fn every_sheet_step_undoes_and_redoes_exactly() {
    let mut book = Workbook::from_sheets(
        vec![
            ("Data".into(), table("a"), Origin::New, Default::default()),
            ("Notes".into(), table("b"), Origin::New, Default::default()),
        ],
        Vec::new(),
    );
    let mut undo: UndoStack<Workbook> = UndoStack::default();
    let mut states = vec![state(&book)];

    // Add a sheet after the first, and edit it.
    let name = book.new_sheet_name();
    assert_eq!(name, "Sheet1");
    let sheet = book.new_sheet(name, CsvTable::untitled(), Origin::New);
    let added = sheet.id;
    let insert = BookEdit::Insert {
        at: 1,
        sheet: Box::new(sheet),
    };
    undo.execute(
        Box::new(SheetEdit::new("Add sheet", vec![insert], Some(added))),
        &mut book,
    );
    book.set_active(added);
    let edit = {
        let t = &mut book.sheet_mut(added).unwrap().table;
        t.insert_rows(0, 1).unwrap()
    };
    states.push(state(&book));
    undo.execute(
        Box::new(OnSheet::new(
            added,
            Box::new(commands::Reshape::new(
                edit,
                CellRef {
                    row: data_model::RowId::source(0),
                    col: data_model::ColId::source(0),
                },
            )),
        )),
        &mut book,
    );
    states.push(state(&book));
    let at = {
        let t = &book.sheet(added).unwrap().table;
        CellRef {
            row: t.row_id(0),
            col: t.col_id(0),
        }
    };
    undo.execute(
        Box::new(OnSheet::new(added, Box::new(SetCell::new(at, "new")))),
        &mut book,
    );
    states.push(state(&book));

    // Rename, move, delete.
    let notes = book.sheets()[2].id;
    let rename = BookEdit::Rename {
        id: notes,
        name: "Q1 Sales".into(),
    };
    undo.execute(
        Box::new(SheetEdit::new("Rename sheet", vec![rename], Some(notes))),
        &mut book,
    );
    states.push(state(&book));
    undo.execute(
        Box::new(SheetEdit::new(
            "Move sheet",
            vec![BookEdit::Move { from: 2, to: 0 }],
            Some(notes),
        )),
        &mut book,
    );
    assert_eq!(book.sheets()[0].name, "Q1 Sales");
    states.push(state(&book));
    let at = book.index_of(added).unwrap();
    undo.execute(
        Box::new(SheetEdit::new(
            "Delete sheet",
            vec![BookEdit::Remove { at }],
            None,
        )),
        &mut book,
    );
    assert!(book.sheet(added).is_none());
    assert_ne!(
        book.active_id(),
        added,
        "deleting the active sheet shows another"
    );
    states.push(state(&book));

    // Undo walks back through every state; redo forward again. The active sheet after an
    // undo is the app's to choose, so only the sheets and their edits are compared.
    let sheets = |s: &State| (s.0.clone(), s.2.clone());
    let steps = undo.len();
    assert_eq!(steps, states.len() - 1, "a state after every step");
    for i in (0..steps).rev() {
        let label = undo.peek_undo().unwrap().label().to_owned();
        undo.undo(&mut book).unwrap();
        assert_eq!(sheets(&state(&book)), sheets(&states[i]), "undo {label}");
    }
    for want in &states[1..] {
        let label = undo.peek_redo().unwrap().label().to_owned();
        undo.redo(&mut book).unwrap();
        assert_eq!(sheets(&state(&book)), sheets(want), "redo {label}");
    }
    // The deleted sheet's edits came back with it on undo, and went again on redo.
    undo.undo(&mut book).unwrap();
    let back = &book.sheet(added).unwrap().table;
    assert_eq!(back.cell_value(0, 0).as_deref(), Some("new"));
}

#[test]
fn a_sheet_step_names_the_sheet_to_show() {
    let book = Workbook::default();
    let id = book.active_id();
    let cmd = SheetEdit::new("Rename sheet", Vec::new(), Some(id));
    assert_eq!(cmd.sheet(), Some(id));
    let cmd = OnSheet::new(
        id,
        Box::new(SetCell::new(
            CellRef {
                row: data_model::RowId::source(0),
                col: data_model::ColId::source(0),
            },
            "x",
        )),
    );
    assert_eq!(cmd.sheet(), Some(id));
    assert_eq!(cmd.label(), "Edit cell");
}

#[test]
fn sheet_names_follow_excels_rules() {
    let book = Workbook::from_sheets(
        vec![
            (
                "Data".into(),
                CsvTable::untitled(),
                Origin::New,
                Default::default(),
            ),
            (
                "Notes".into(),
                CsvTable::untitled(),
                Origin::New,
                Default::default(),
            ),
        ],
        Vec::new(),
    );
    let data = book.sheets()[0].id;
    let cases: &[(&str, Option<SheetId>, Result<(), &str>)] = &[
        ("Q1 Sales", None, Ok(())),
        ("", None, Err("A sheet name can't be empty")),
        ("   ", None, Err("A sheet name can't be empty")),
        ("1234567890123456789012345678901", None, Ok(())),
        (
            "12345678901234567890123456789012",
            None,
            Err("A sheet name has at most 31 characters"),
        ),
        (
            "a/b",
            None,
            Err("A sheet name can't contain [ ] : * ? / \\"),
        ),
        (
            "a[1]",
            None,
            Err("A sheet name can't contain [ ] : * ? / \\"),
        ),
        (
            "a:b",
            None,
            Err("A sheet name can't contain [ ] : * ? / \\"),
        ),
        (
            "why?",
            None,
            Err("A sheet name can't contain [ ] : * ? / \\"),
        ),
        (
            "a\\b",
            None,
            Err("A sheet name can't contain [ ] : * ? / \\"),
        ),
        (
            "star*",
            None,
            Err("A sheet name can't contain [ ] : * ? / \\"),
        ),
        (
            "'quoted",
            None,
            Err("A sheet name can't start or end with '"),
        ),
        (
            "quoted'",
            None,
            Err("A sheet name can't start or end with '"),
        ),
        ("It's", None, Ok(())),
        ("notes", None, Err("Another sheet has that name")),
        ("DATA", Some(data), Ok(())),
        ("Données", None, Ok(())),
    ];
    for &(name, except, want) in cases {
        assert_eq!(book.check_sheet_name(name, except), want, "{name:?}");
    }
    assert_eq!(book.new_sheet_name(), "Sheet1");
}
