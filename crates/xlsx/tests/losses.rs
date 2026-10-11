//! XLSX-2: Bricks names the workbook content a save drops, and nothing for what it keeps.

use data_model::{CsvTable, DelimiterChoice, Origin, SaveProgress, Workbook};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use xlsx::losses::scan;
use xlsx::{import, save_xlsx, ExportSheet, Loss, Losses, Progress};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/fixture.xlsx")
}

#[test]
fn the_fixture_names_its_formatting_chart_comment_and_merged_cells() {
    let expected = Losses {
        items: vec![
            (Loss::CellFormatting, 1),
            (Loss::Charts, 1),
            (Loss::Comments, 1),
            (Loss::MergedCells, 1),
            (Loss::ConditionalFormatting, 1),
            (Loss::DataValidation, 1),
            (Loss::Hyperlinks, 1),
        ],
    };
    assert_eq!(scan(&fixture()).unwrap(), expected);
    let imported = import(&fixture(), &AtomicBool::new(false), &Progress::default()).unwrap();
    assert_eq!(imported.losses, expected);
    assert_eq!(
        expected.sentence(),
        "cell formatting, 1 chart, comments, merged cells, conditional formatting, \
         data validation, hyperlinks"
    );
}

#[test]
fn a_workbook_bricks_saved_loses_nothing() {
    let rows = Path::new(ROOT).join("corpus/rows_10mb.csv");
    if !rows.exists() {
        eprintln!(
            "skipped: {} is missing (python3 scripts/gen_corpus.py)",
            rows.display()
        );
        return;
    }
    let table = CsvTable::open(&rows, DelimiterChoice::Auto).unwrap();
    table.index().build(&AtomicBool::new(false)).unwrap();
    let book = Workbook::single("rows".into(), table, Origin::Csv);
    let sheet = book.active_sheet();
    let sheets = vec![ExportSheet {
        name: sheet.name.clone(),
        table: sheet.table.snapshot(),
        computed: Default::default(),
        widths: vec![(0, 120)],
        active: true,
    }];
    let dest = std::env::temp_dir().join(format!("bricks-losses-{}.xlsx", std::process::id()));
    save_xlsx(
        &dest,
        sheets,
        &[],
        &SaveProgress::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let losses = scan(&dest).unwrap();
    std::fs::remove_file(&dest).unwrap();
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(losses.sentence(), "");
}

#[test]
fn sentences_pluralize_only_charts() {
    let losses = Losses {
        items: vec![
            (Loss::Charts, 3),
            (Loss::Images, 2),
            (Loss::ExternalLinks, 1),
            (Loss::HiddenSheets, 1),
        ],
    };
    assert_eq!(
        losses.sentence(),
        "3 charts, images, links to other files, hidden sheets (shown)"
    );
    assert_eq!(Loss::Charts.text(1), "1 chart");
    assert!(!losses.is_empty());
}
