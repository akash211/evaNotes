use crate::db::shares;
use crate::models::note::NoteType;
use crate::services::markdown::render_markdown_to_html;
use crate::services::pdf::note_to_pdf_bytes;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use std::fmt::Write as _;

const PAGE_CSS: &str = r#"
:root { color-scheme: light dark; }
body { font-family: 'Segoe UI', system-ui, -apple-system, sans-serif; max-width: 780px;
       margin: 0 auto; padding: 40px 20px 80px; line-height: 1.65; }
.badge { display: inline-block; font-size: 11px; font-weight: 700; letter-spacing: .06em;
         text-transform: uppercase; padding: 2px 10px; border-radius: 20px; margin-bottom: 14px; }
.badge-live { background: #dcfce7; color: #166534; }
.badge-snapshot { background: #fef3c7; color: #92400e; }
h1 { font-size: 30px; margin: 0 0 6px; }
.meta { color: #71717a; font-size: 12.5px; margin-bottom: 26px; }
.content { font-size: 15.5px; }
.content pre { background: #0d1117; color: #e6edf3; padding: 14px 16px; border-radius: 10px;
               overflow-x: auto; font-size: 13px; }
.content code { background: rgba(127,127,127,.18); padding: 1px 6px; border-radius: 5px;
                font-size: 13.5px; }
.content pre code { background: none; padding: 0; }
.content blockquote { border-left: 3px solid #8b8b93; margin: 0; padding: 2px 16px; color: #666; }
.content img { max-width: 100%; border-radius: 8px; }
.content table { border-collapse: collapse; width: 100%; }
.content th, .content td { border: 1px solid #8882; padding: 6px 10px; font-size: 14px; }
.content th { background: #8881; }
.content a { color: #3b82f6; }
.actions { position: sticky; bottom: 0; display: flex; gap: 10px; flex-wrap: wrap;
           margin-top: 34px; padding: 14px 0; }
.btn { display: inline-block; text-decoration: none; font-weight: 600; font-size: 14px;
       padding: 9px 18px; border-radius: 10px; border: 1px solid transparent; }
.btn-pdf { background: #2563eb; color: #fff; }
.btn-md { background: transparent; border-color: #8885; color: inherit; }
.footer { margin-top: 40px; font-size: 12px; color: #999; }
@media (prefers-color-scheme: dark) {
  body { background: #0b0e17; color: #e7eaf3; }
  .meta { color: #8a93ab; }
  .content blockquote { color: #9aa3b8; }
  .btn-md { border-color: #3a4160; }
  .footer { color: #6d7794; }
}
"#;

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn resolve(
    state: &AppState,
    slug: &str,
) -> Result<Option<(String, String, String, String)>, crate::error::AppError> {
    let conn = state.pool.get()?;
    shares::resolve_share(&conn, slug)
}

pub async fn share_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response, crate::error::AppError> {
    match resolve(&state, &slug).await? {
        None => Ok((
            StatusCode::NOT_FOUND,
            Html(
                "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Not found</title></head>\
                 <body style=\"font-family:sans-serif;padding:40px;text-align:center\">\
                 <h2>Shared note not found</h2><p>This link may have been revoked.</p></body></html>"
                    .to_string(),
            ),
        )
            .into_response()),
        Some((mode, title, content, note_type)) => {
            let is_live = mode == "live";
            let badge = if is_live {
                r#"<span class="badge badge-live">Live</span>"#
            } else {
                r#"<span class="badge badge-snapshot">Snapshot</span>"#
            };
            let type_label = NoteType::from_str_opt(&note_type);

            let body = match type_label {
                NoteType::Detailed => render_markdown_to_html(&content),
                NoteType::Code => crate::services::markdown::render_code_to_html(&content, "text"),
                NoteType::Sheet => render_sheet_table(&content),
                NoteType::Simple => {
                    format!("<p>{}</p>", html_escape(&content).replace('\n', "<br>"))
                }
            };

            let html = format!(
                r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>{css}</style>
</head>
<body>
  {badge}
  <h1>{title}</h1>
  <div class="meta">Shared from evaNotes · {mode} share</div>
  <div class="content">{body}</div>
  <div class="actions">
    <a class="btn btn-pdf" href="/share/{slug}/pdf" download>⬇ Download PDF</a>
    <a class="btn btn-md" href="/share/{slug}/md" download>Download Markdown</a>
  </div>
  <div class="footer">Powered by evaNotes — a self-hosted notes &amp; reminders app.</div>
</body>
</html>"#,
                title = html_escape(&title),
                css = PAGE_CSS,
                badge = badge,
                mode = if is_live { "always updated" } else { "frozen copy" },
                body = body,
                slug = slug,
            );

            Ok(Html(html).into_response())
        }
    }
}

fn render_sheet_table(content: &str) -> String {
    let rows: Vec<Vec<String>> = serde_json::from_str(content).unwrap_or_default();
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::from("<table>");
    for (ri, row) in rows.iter().enumerate() {
        if ri == 0 {
            out.push_str("<thead><tr>");
            for cell in row {
                let _ = write!(out, "<th>{}</th>", html_escape(cell));
            }
            out.push_str("</tr></thead><tbody>");
        } else {
            out.push_str("<tr>");
            for cell in row {
                let _ = write!(out, "<td>{}</td>", html_escape(cell));
            }
            out.push_str("</tr>");
        }
    }
    out.push_str("</tbody></table>");
    out
}

fn share_attachment_headers(filename: &str, content_type: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", filename.replace('"', ""))
            .parse()
            .unwrap(),
    );
    headers
}

pub async fn share_markdown(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response, crate::error::AppError> {
    match resolve(&state, &slug).await? {
        None => Ok(StatusCode::NOT_FOUND.into_response()),
        Some((_mode, title, content, _note_type)) => {
            let filename = format!(
                "{}.md",
                crate::services::markdown::sanitize_filename(&title)
            );
            let headers = share_attachment_headers(&filename, "text/markdown; charset=utf-8");
            Ok((StatusCode::OK, headers, content).into_response())
        }
    }
}

pub async fn share_pdf(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response, crate::error::AppError> {
    match resolve(&state, &slug).await? {
        None => Ok(StatusCode::NOT_FOUND.into_response()),
        Some((_mode, title, content, note_type)) => {
            let note = crate::db::shares::share_note_view(title, content, note_type);
            let pdf = note_to_pdf_bytes(&note);
            let filename = format!(
                "{}.pdf",
                crate::services::markdown::sanitize_filename(&note.title)
            );
            let headers = share_attachment_headers(&filename, "application/pdf");
            Ok((StatusCode::OK, headers, pdf).into_response())
        }
    }
}
