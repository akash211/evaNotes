use crate::models::note::{Note, NoteType};
use crate::services::markdown::render_markdown_to_text;
use genpdf::fonts::{FontData, FontFamily};
use genpdf::style::{Color, Style};
use genpdf::{elements, Document, SimplePageDecorator};

const REGULAR: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans.ttf");
const BOLD: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf");

fn font_family() -> FontFamily<FontData> {
    let regular = FontData::new(REGULAR.to_vec(), None).expect("embedded regular font");
    let bold = FontData::new(BOLD.to_vec(), None).expect("embedded bold font");
    FontFamily {
        regular: regular.clone(),
        bold: bold.clone(),
        italic: regular.clone(),
        bold_italic: bold,
    }
}

/// Remove control characters that would break PDF text layout.
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// Sheet notes store content as JSON rows; convert to aligned text columns.
fn sheet_to_text(content: &str) -> String {
    let rows: Vec<Vec<String>> = serde_json::from_str(content).unwrap_or_default();
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1);
    let mut widths = vec![0usize; cols];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in &rows {
        let mut line = String::new();
        for (i, width) in widths.iter().enumerate() {
            let cell = row.get(i).map(|c| c.as_str()).unwrap_or("");
            line.push_str(cell);
            if i + 1 < cols {
                let pad = width.saturating_sub(cell.chars().count()) + 2;
                line.push_str(&" ".repeat(pad));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Render a note to PDF bytes (embedded DejaVu fonts, Unicode-friendly).
pub fn note_to_pdf_bytes(note: &Note) -> Vec<u8> {
    let mut doc = Document::new(font_family());
    let mut decorator = SimplePageDecorator::new();
    decorator.set_margins(28);
    doc.set_page_decorator(decorator);

    doc.set_title(sanitize(&note.title));

    // Title
    let mut title_style = Style::new();
    title_style.set_font_size(17);
    title_style.set_bold();
    doc.push(elements::Text::new(genpdf::style::StyledString::new(
        sanitize(&note.title),
        title_style,
    )));
    doc.push(elements::Break::new(2));

    // Metadata line
    let mut meta_style = Style::new();
    meta_style.set_font_size(8);
    meta_style.set_color(Color::Rgb(110, 110, 130));
    let mut meta = format!(
        "Type: {}   |   Updated: {}",
        note.note_type.as_str(),
        note.updated_at
    );
    if !note.tags.is_empty() {
        meta.push_str(&format!("   |   Tags: {}", note.tags.join(", ")));
    }
    doc.push(elements::Text::new(genpdf::style::StyledString::new(
        sanitize(&meta),
        meta_style,
    )));
    doc.push(elements::Break::new(2));

    // Body
    let mut body_style = Style::new();
    body_style.set_font_size(10);
    let text = match note.note_type {
        NoteType::Detailed => render_markdown_to_text(&note.content),
        NoteType::Code => note.content.clone(),
        NoteType::Sheet => sheet_to_text(&note.content),
        NoteType::Simple => note.content.clone(),
    };

    for line in sanitize(&text).split('\n') {
        if line.trim().is_empty() {
            doc.push(elements::Break::new(1));
        } else {
            doc.push(elements::Text::new(genpdf::style::StyledString::new(
                line.to_string(),
                body_style,
            )));
        }
    }

    let mut buf = Vec::new();
    if doc.render(&mut buf).is_err() {
        return Vec::new();
    }
    buf
}
