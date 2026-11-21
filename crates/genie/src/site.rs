//! The web site in `docs/`: the pages of `site/` in one layout, a manual from the README, the
//! changelog, the keys of the app and its logo. The test at the end compares them with the files
//! in `docs/`; `GENIE_WRITE_SITE=1 cargo test -p genie --bin genie site` writes them.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::brand;
use crate::help;
use crate::saved::Saved;

const SITE: &str = "https://collide2954.github.io/genie/";
const REPOSITORY: &str = "https://github.com/collide2954/genie";

/// The items of the menu at the top of each page, with their pages.
const MENU: [(&str, &str); 6] = [
    ("Tutorial", "tutorial.html"),
    ("Manual", "manual/index.html"),
    ("Examples", "examples.html"),
    ("Keys", "keys.html"),
    ("Changelog", "changelog.html"),
    ("FAQ", "faq.html"),
];

/// The pages of the manual: a file name, a title and the sections of the README in the page.
const MANUAL: [(&str, &str, &[&str]); 9] = [
    ("install", "Install", &["Install", "Build from source"]),
    (
        "genomes",
        "Genomes and tracks",
        &["Genomes", "UCSC tracks", "Track hubs"],
    ),
    ("files", "Files", &["Files", "Files on the web"]),
    (
        "navigation",
        "Navigation",
        &[
            "Navigation",
            "Find a sequence",
            "Several loci",
            "Whole genome",
            "Regions",
            "Copy",
        ],
    ),
    (
        "tracks",
        "Tracks",
        &[
            "Tracks",
            "Variant table",
            "B-allele frequency",
            "Depth ratio",
            "Variant review",
            "LD",
            "Sashimi plots",
            "Isoforms",
            "Isoform table",
            "Splicing differences",
            "Phase reads",
            "Single-cell",
            "Reference tracks",
            "Motif sites",
            "Dot plot",
            "Liftover",
            "Sample groups",
        ],
    ),
    (
        "sessions",
        "Sessions and images",
        &["Undo", "Sessions", "Export", "Images from the command line"],
    ),
    (
        "automation",
        "Automation",
        &["Commands from other programs"],
    ),
    (
        "settings",
        "Settings and updates",
        &["Settings", "Updates", "Diagnostics"],
    ),
    (
        "development",
        "Development",
        &["Test", "Website", "Crates", "License"],
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A page of the site: its path in `docs/`, its title, description and menu item, and its body.
struct Page {
    path: String,
    title: String,
    description: String,
    menu: String,
    body: String,
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn layout(page: &Page) -> String {
    let up = "../".repeat(page.path.matches('/').count());
    let mut menu = String::new();
    for (label, path) in MENU {
        let current = if label == page.menu {
            " aria-current=\"page\""
        } else {
            ""
        };
        let _ = write!(menu, "\n      <a href=\"{up}{path}\"{current}>{label}</a>");
    }
    let home = page.path == "index.html";
    let title = if home {
        page.title.clone()
    } else {
        format!("{} · genie", page.title)
    };
    let address = if home {
        SITE.to_owned()
    } else {
        format!("{SITE}{}", page.path)
    };
    let download = if home {
        "#download".to_owned()
    } else {
        format!("{up}index.html#download")
    };
    let (title, description) = (escape(&title), escape(&page.description));
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:type" content="website">
<meta property="og:url" content="{address}">
<meta property="og:image" content="{SITE}images/card.png">
<meta name="twitter:card" content="summary_large_image">
<meta name="theme-color" content="#F8F8F8" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#181818" media="(prefers-color-scheme: dark)">
<link rel="icon" href="{up}images/logo.svg" type="image/svg+xml">
<link rel="stylesheet" href="{up}style.css">
<script src="{up}site.js" defer></script>
</head>
<body>
<header class="bar">
  <div class="wrap">
    <a class="brand" href="{up}index.html"><img src="{up}images/logo.svg" width="18" height="18" alt="">genie</a>
    <nav aria-label="Site">{menu}
      <a href="{REPOSITORY}">GitHub</a>
    </nav>
    <a class="button primary small" href="{download}">Download</a>
  </div>
</header>
<main>
{body}
</main>
<footer>
  <div class="wrap">
    <p>genie uses the Apache-2.0 license. The fonts Inter and JetBrains Mono use the SIL Open Font License 1.1.</p>
    <p>The pictures show data from the UCSC Genome Browser, ClinVar, gnomAD, ENCODE, the GWAS Catalog, the 1000 Genomes Project and the JBrowse demo data.</p>
  </div>
</footer>
</body>
</html>
"##,
        body = page.body.trim_end()
    )
}

/// The id of a heading, as GitHub makes it: lower case, words joined with hyphens.
fn slug(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter_map(|letter| match letter {
            ' ' | '-' => Some('-'),
            _ if letter.is_alphanumeric() || letter == '_' => Some(letter),
            _ => None,
        })
        .collect()
}

/// Inline Markdown: code spans, bold text and links. `link` gives the address of a link.
fn inline(text: &str, link: &dyn Fn(&str) -> String) -> String {
    let mut html = String::new();
    for (index, part) in text.split('`').enumerate() {
        if index % 2 == 1 {
            let _ = write!(html, "<code>{}</code>", escape(part));
            continue;
        }
        let mut rest = part;
        while let Some(open) = rest.find('[') {
            let target = rest[open..]
                .find("](")
                .map(|at| open + at)
                .and_then(|middle| {
                    let close = rest[middle..].find(')')? + middle;
                    Some((middle, close))
                });
            let Some((middle, close)) = target else {
                break;
            };
            html.push_str(&bold(&escape(&rest[..open])));
            let words = bold(&escape(&rest[open + 1..middle]));
            let address = escape(&link(&rest[middle + 2..close]));
            let _ = write!(html, "<a href=\"{address}\">{words}</a>");
            rest = &rest[close + 1..];
        }
        html.push_str(&bold(&escape(rest)));
    }
    html
}

fn bold(text: &str) -> String {
    let parts: Vec<&str> = text.split("**").collect();
    if parts.len().is_multiple_of(2) {
        return text.to_owned();
    }
    let mut html = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index % 2 == 1 {
            let _ = write!(html, "<b>{part}</b>");
        } else {
            html.push_str(part);
        }
    }
    html
}

/// The cells of a row of a Markdown table, split at the bars outside code spans.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim().trim_start_matches('|').trim_end_matches('|');
    let mut cells = vec![String::new()];
    let mut code = false;
    for letter in line.chars() {
        match letter {
            '`' => {
                code = !code;
                cells.last_mut().unwrap().push(letter);
            }
            '|' if !code => cells.push(String::new()),
            _ => cells.last_mut().unwrap().push(letter),
        }
    }
    cells
        .into_iter()
        .map(|cell| cell.trim().to_owned())
        .collect()
}

fn numbered(line: &str) -> Option<&str> {
    let (number, rest) = line.split_once(". ")?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then_some(rest)
}

fn starts_block(line: &str) -> bool {
    line.starts_with('#')
        || line.starts_with("```")
        || line.starts_with('|')
        || line.starts_with("- ")
        || numbered(line).is_some()
}

/// Markdown blocks: headings with ids, paragraphs, lists, tables and code blocks. `heading`
/// can give the HTML of a heading of a level and a text.
fn markdown(
    text: &str,
    href: &dyn Fn(&str) -> String,
    heading: &dyn Fn(usize, &str) -> Option<String>,
) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut html = String::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim().is_empty() {
            at += 1;
        } else if line.starts_with("```") {
            let mut code = Vec::new();
            at += 1;
            while at < lines.len() && !lines[at].starts_with("```") {
                code.push(lines[at]);
                at += 1;
            }
            at += 1;
            let _ = writeln!(html, "<pre><code>{}</code></pre>", escape(&code.join("\n")));
        } else if line.starts_with('#') {
            let level = line.chars().take_while(|letter| *letter == '#').count();
            let title = line[level..].trim();
            let made = heading(level, title).unwrap_or_else(|| {
                format!(
                    "<h{level} id=\"{}\">{}</h{level}>",
                    slug(title),
                    inline(title, href)
                )
            });
            html.push_str(&made);
            html.push('\n');
            at += 1;
        } else if line.starts_with('|') {
            let mut rows = Vec::new();
            while at < lines.len() && lines[at].starts_with('|') {
                rows.push(cells(lines[at]));
                at += 1;
            }
            html.push_str("<div class=\"table\"><table>\n<thead><tr>");
            for cell in &rows[0] {
                let _ = write!(html, "<th>{}</th>", inline(cell, href));
            }
            html.push_str("</tr></thead>\n<tbody>\n");
            for row in rows.iter().skip(2) {
                html.push_str("<tr>");
                for cell in row {
                    let _ = write!(html, "<td>{}</td>", inline(cell, href));
                }
                html.push_str("</tr>\n");
            }
            html.push_str("</tbody></table></div>\n");
        } else if line.starts_with("- ") || numbered(line).is_some() {
            let ordered = numbered(line).is_some();
            let tag = if ordered { "ol" } else { "ul" };
            let _ = writeln!(html, "<{tag}>");
            while at < lines.len() {
                let item = if ordered {
                    numbered(lines[at])
                } else {
                    lines[at].strip_prefix("- ")
                };
                let Some(item) = item else {
                    break;
                };
                let _ = writeln!(html, "<li>{}</li>", inline(item, href));
                at += 1;
            }
            let _ = writeln!(html, "</{tag}>");
        } else {
            let mut words = Vec::new();
            while at < lines.len() && !lines[at].trim().is_empty() && !starts_block(lines[at]) {
                words.push(lines[at].trim());
                at += 1;
            }
            let _ = writeln!(html, "<p>{}</p>", inline(&words.join(" "), href));
        }
    }
    html
}

/// The text of Markdown without its marks, for the search.
fn plain(text: &str) -> String {
    let mut words = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("| ---") || line.is_empty() {
            continue;
        }
        let mut line = line.replace(['`', '|'], " ").replace("**", "");
        while let (Some(open), Some(middle)) = (line.find('['), line.find("](")) {
            let Some(close) = line[middle..].find(')').map(|at| at + middle) else {
                break;
            };
            if open > middle {
                break;
            }
            line = format!(
                "{}{}{}",
                &line[..open],
                &line[open + 1..middle],
                &line[close + 1..]
            );
        }
        words.push_str(line.trim_start_matches(['#', '-', ' ']));
        words.push(' ');
    }
    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for letter in text.chars() {
        match letter {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            letter if u32::from(letter) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(letter));
            }
            letter => out.push(letter),
        }
    }
    out.push('"');
    out
}

/// The sections of the README: the title of each `##` section and its Markdown.
fn sections(readme: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for line in readme.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            found.push((title.trim().to_owned(), String::new()));
        } else if let Some((_, text)) = found.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    found
}

/// The pages of the manual, its index page and its search index.
#[allow(clippy::too_many_lines)]
fn manual(readme: &str) -> (Vec<Page>, String) {
    let all = sections(readme);
    let page_of = |title: &str| {
        MANUAL
            .iter()
            .find(|(_, _, titles)| titles.contains(&title))
            .map(|(file, _, _)| *file)
    };
    // The page of each heading id, so that links to a section go to its page.
    let mut places: Vec<(String, &str)> = Vec::new();
    for (title, text) in &all {
        let Some(file) = page_of(title) else {
            continue;
        };
        places.push((slug(title), file));
        for line in text.lines() {
            if let Some(sub) = line.strip_prefix("### ") {
                places.push((slug(sub.trim()), file));
            }
        }
    }
    let mut pages = Vec::new();
    let mut index = Vec::new();
    for (file, page_title, titles) in MANUAL {
        let link = |target: &str| -> String {
            if let Some(anchor) = target.strip_prefix('#') {
                match places.iter().find(|(id, _)| id == anchor) {
                    Some((_, place)) if *place != file => format!("{place}.html#{anchor}"),
                    _ => target.to_owned(),
                }
            } else if target.starts_with("http://") || target.starts_with("https://") {
                target.to_owned()
            } else {
                format!("{REPOSITORY}/blob/main/{target}")
            }
        };
        let mut article = String::new();
        let mut contents = String::new();
        // A page named as its first section has one heading for both, with the id of the section.
        let lead = titles.first() == Some(&page_title);
        for (place, title) in titles.iter().enumerate() {
            let Some((_, text)) = all.iter().find(|(found, _)| found.as_str() == *title) else {
                continue;
            };
            if !(lead && place == 0) {
                let _ = writeln!(article, "<h2 id=\"{}\">{}</h2>", slug(title), escape(title));
            }
            article.push_str(&markdown(text, &link, &|_, _| None));
            let _ = writeln!(
                contents,
                "          <li><a href=\"#{}\">{}</a></li>",
                slug(title),
                escape(title)
            );
            let mut parts = vec![(title.to_string(), slug(title), String::new())];
            for line in text.lines() {
                if let Some(sub) = line.strip_prefix("### ") {
                    parts.push((sub.trim().to_owned(), slug(sub.trim()), String::new()));
                } else if let Some((_, _, part)) = parts.last_mut() {
                    part.push_str(line);
                    part.push('\n');
                }
            }
            for (part_title, id, part) in parts {
                index.push(format!(
                    "{{\"title\":{},\"page\":{},\"url\":{},\"text\":{}}}",
                    json(&part_title),
                    json(page_title),
                    json(&format!("{file}.html#{id}")),
                    json(&plain(&part))
                ));
            }
        }
        let id = if lead {
            format!(" id=\"{}\"", slug(page_title))
        } else {
            String::new()
        };
        let body = format!(
            "<div class=\"wrap manual\">\n{}\n<article class=\"prose\">\n<h1{id}>{}</h1>\n{}</article>\n</div>",
            sidebar(Some((file, &contents))),
            escape(page_title),
            article
        );
        pages.push(Page {
            path: format!("manual/{file}.html"),
            title: page_title.to_owned(),
            description: format!("The genie manual: {}.", titles.join(", ").to_lowercase()),
            menu: "Manual".to_owned(),
            body,
        });
    }
    let mut list = String::new();
    for (file, page_title, titles) in MANUAL {
        let _ = writeln!(
            list,
            "<h2><a href=\"{file}.html\">{}</a></h2>\n<p class=\"muted\">{}</p>",
            escape(page_title),
            escape(&titles.join(" · "))
        );
    }
    let first = readme
        .lines()
        .skip_while(|line| line.starts_with('#') || line.trim().is_empty())
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let link = |target: &str| target.to_owned();
    let body = format!(
        "<div class=\"wrap manual\">\n{}\n<article class=\"prose\">\n<h1>Manual</h1>\n<p>{}</p>\n<p>Type words in the box to find a section, or open a page.</p>\n{}</article>\n</div>",
        sidebar(None),
        inline(&first, &link),
        list
    );
    pages.insert(
        0,
        Page {
            path: "manual/index.html".to_owned(),
            title: "Manual".to_owned(),
            description: "The genie manual: genomes, files, navigation, tracks, sessions, images and automation.".to_owned(),
            menu: "Manual".to_owned(),
            body,
        },
    );
    (pages, format!("[\n{}\n]\n", index.join(",\n")))
}

/// The side menu of the manual, with the search box and the sections of the current page.
fn sidebar(current: Option<(&str, &str)>) -> String {
    let mut html = String::from(
        "<aside>\n  <input class=\"search\" type=\"search\" placeholder=\"Find in the manual\" aria-label=\"Find in the manual\" data-index=\"search.json\">\n  <ul class=\"results\"></ul>\n  <nav aria-label=\"Manual\">\n    <ul>\n",
    );
    for (file, title, _) in MANUAL {
        match current {
            Some((page, contents)) if page == file => {
                let _ = write!(
                    html,
                    "      <li><a href=\"{file}.html\" aria-current=\"page\">{}</a>\n        <ul>\n{contents}        </ul>\n      </li>\n",
                    escape(title)
                );
            }
            _ => {
                let _ = writeln!(
                    html,
                    "      <li><a href=\"{file}.html\">{}</a></li>",
                    escape(title)
                );
            }
        }
    }
    html.push_str("    </ul>\n  </nav>\n</aside>");
    html
}

/// The changelog, with a link to the release page of each version.
fn changelog(text: &str) -> Page {
    let link = |target: &str| target.to_owned();
    let heading = |level: usize, title: &str| -> Option<String> {
        match level {
            1 => Some(String::new()),
            2 if title
                .chars()
                .next()
                .is_some_and(|letter| letter.is_ascii_digit()) =>
            {
                Some(format!(
                    "<h2 id=\"v{title}\">{title} <a href=\"{REPOSITORY}/releases/tag/v{title}\">Release page</a></h2>"
                ))
            }
            _ => None,
        }
    };
    let body = format!(
        "<div class=\"wrap page-head\">\n<h1>Changelog</h1>\n<p>What changed in each version of genie. The <a href=\"{REPOSITORY}/releases\">release pages</a> have the packages.</p>\n</div>\n<div class=\"wrap prose release\">\n{}</div>",
        markdown(text, &link, &heading)
    );
    Page {
        path: "changelog.html".to_owned(),
        title: "Changelog".to_owned(),
        description: "What changed in each version of genie.".to_owned(),
        menu: "Changelog".to_owned(),
        body,
    }
}

/// The keys of a row for Windows and Linux, and for macOS, from the keys of this build.
fn both_keys(keys: &str) -> (String, String) {
    let others = keys
        .replace("Cmd+[, Cmd+]", "Alt+Left, Alt+Right")
        .replace("Cmd", "Ctrl");
    let mac = others
        .replace("Alt+Left, Alt+Right", "Cmd+[, Cmd+]")
        .replace("Ctrl", "Cmd");
    (others, mac)
}

fn keys_page() -> Page {
    let mut groups = String::new();
    for group in help::groups() {
        let _ = write!(
            groups,
            "<div>\n<h2>{}</h2>\n<div class=\"table\"><table>\n<thead><tr><th>Action</th><th>Windows and Linux</th><th>macOS</th></tr></thead>\n<tbody>\n",
            escape(group.title)
        );
        for (keys, action) in group.rows {
            let (others, mac) = both_keys(&keys);
            // Words such as "Drag the tracks" are mouse actions: they show in the text font.
            let mouse = keys
                .split_whitespace()
                .any(|word| word.chars().all(|letter| letter.is_ascii_lowercase()));
            let class = if mouse { "" } else { " class=\"mono\"" };
            if others == mac {
                let _ = writeln!(
                    groups,
                    "<tr><td>{}</td><td colspan=\"2\"{class}>{}</td></tr>",
                    escape(action),
                    escape(&others)
                );
            } else {
                let _ = writeln!(
                    groups,
                    "<tr><td>{}</td><td{class}>{}</td><td{class}>{}</td></tr>",
                    escape(action),
                    escape(&others),
                    escape(&mac)
                );
            }
        }
        groups.push_str("</tbody></table></div>\n</div>\n");
    }
    let body = format!(
        "<div class=\"wrap page-head\">\n<h1>Keys and mouse</h1>\n<p>All keys and mouse actions of genie. Push F1 in genie to see this list. Ctrl+K (Cmd+K on macOS) opens a palette that finds any action by its words.</p>\n</div>\n<div class=\"wrap keys\">\n{groups}</div>"
    );
    Page {
        path: "keys.html".to_owned(),
        title: "Keys and mouse".to_owned(),
        description: "All keys and mouse actions of genie, for Windows, Linux and macOS."
            .to_owned(),
        menu: "Keys".to_owned(),
        body,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn hex(color: iced::Color) -> String {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        byte(color.r),
        byte(color.g),
        byte(color.b)
    )
}

/// The icon of genie as an SVG image, drawn from the same strands as in the app.
fn logo() -> String {
    let look = brand::ICON;
    let path = |points: &[[f32; 2]]| {
        let mut data = String::new();
        for (index, [x, y]) in points.iter().enumerate() {
            let command = if index == 0 { 'M' } else { 'L' };
            let _ = write!(data, "{command}{x:.2} {y:.2}");
        }
        data
    };
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {grid} {grid}\" width=\"{grid}\" height=\"{grid}\">\n<rect width=\"{grid}\" height=\"{grid}\" rx=\"5.5\" fill=\"{square}\"/>\n<g fill=\"none\">\n",
        grid = brand::GRID,
        square = hex(look.square)
    );
    let glyph = hex(look.glyph);
    for [from, to] in brand::rungs() {
        let _ = writeln!(
            svg,
            "<path d=\"{}\" stroke=\"{glyph}\" stroke-width=\"1\" stroke-linecap=\"round\"/>",
            path(&[from, to])
        );
    }
    let parts = brand::bases(-1.0);
    for (base, points) in [parts.first(), parts.last()].into_iter().flatten() {
        let _ = writeln!(
            svg,
            "<path d=\"{}\" stroke=\"{}\" stroke-width=\"1.75\" stroke-linecap=\"round\"/>",
            path(points),
            hex(look.bases[*base])
        );
    }
    for (base, points) in &parts {
        let _ = writeln!(
            svg,
            "<path d=\"{}\" stroke=\"{}\" stroke-width=\"1.75\"/>",
            path(points),
            hex(look.bases[*base])
        );
    }
    let _ = writeln!(
        svg,
        "<path d=\"{}\" stroke=\"{glyph}\" stroke-width=\"1.75\" stroke-linecap=\"round\"/>",
        path(&brand::strand(1.0))
    );
    svg.push_str("</g>\n</svg>\n");
    svg
}

/// A page of `site/`: the comments at its top give its title, description and menu item.
fn source_page(name: &str, text: &str) -> Page {
    let meta = |key: &str| {
        text.lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("<!-- ")?
                    .strip_suffix(" -->")?
                    .strip_prefix(key)?
                    .strip_prefix(": ")
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    };
    let body: Vec<&str> = text
        .lines()
        .skip_while(|line| line.trim_start().starts_with("<!-- "))
        .collect();
    Page {
        path: name.to_owned(),
        title: meta("title"),
        description: meta("description"),
        menu: meta("menu"),
        body: body.join("\n"),
    }
}

/// Every file of the site that this module makes, as a path in `docs/` and its bytes.
fn build() -> Vec<(String, Vec<u8>)> {
    let root = root();
    let read = |path: &Path| fs::read_to_string(root.join(path)).unwrap();
    let mut files = Vec::new();
    for name in ["index.html", "tutorial.html", "examples.html", "faq.html"] {
        let page = source_page(name, &read(&Path::new("site").join(name)));
        files.push((name.to_owned(), layout(&page).into_bytes()));
    }
    let (pages, index) = manual(&read(Path::new("README.md")));
    for page in pages
        .iter()
        .chain([changelog(&read(Path::new("CHANGELOG.md"))), keys_page()].iter())
    {
        files.push((page.path.clone(), layout(page).into_bytes()));
    }
    files.push(("manual/search.json".to_owned(), index.into_bytes()));
    for name in ["style.css", "site.js"] {
        files.push((
            name.to_owned(),
            read(&Path::new("site").join(name)).into_bytes(),
        ));
    }
    files.push(("images/logo.svg".to_owned(), logo().into_bytes()));
    let examples = root.join("site").join("examples");
    let mut sessions: Vec<PathBuf> = fs::read_dir(&examples)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    sessions.sort();
    for session in sessions {
        let name = session.file_name().unwrap().to_string_lossy().into_owned();
        files.push((format!("examples/{name}"), fs::read(&session).unwrap()));
    }
    files
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{Saved, both_keys, build, inline, markdown, plain, root, slug};

    #[test]
    fn markdown_gives_headings_lists_tables_and_code() {
        let link = |target: &str| format!("/{target}");
        let html = markdown(
            "## Files on the web\n\nOpen **one** `a<b>` file.\nSee [x](y).\n\n- a\n- b\n\n1. c\n2. d\n\n| A | B |\n| --- | --- |\n| `x|y` | z |\n\n```bash\nrun <it>\n```\n",
            &link,
            &|_, _| None,
        );
        assert_eq!(
            html,
            "<h2 id=\"files-on-the-web\">Files on the web</h2>\n<p>Open <b>one</b> <code>a&lt;b&gt;</code> file. See <a href=\"/y\">x</a>.</p>\n<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n<ol>\n<li>c</li>\n<li>d</li>\n</ol>\n<div class=\"table\"><table>\n<thead><tr><th>A</th><th>B</th></tr></thead>\n<tbody>\n<tr><td><code>x|y</code></td><td>z</td></tr>\n</tbody></table></div>\n<pre><code>run &lt;it&gt;</code></pre>\n"
        );
        assert_eq!(slug("Hi-C contact maps (v8)"), "hi-c-contact-maps-v8");
        assert_eq!(inline("2 ** 3", &link), "2 ** 3");
        assert_eq!(
            plain("## A\n\n**B** `c` [d](e) | f |\n```\ng\n```"),
            "A B c d f g"
        );
    }

    #[test]
    fn keys_show_for_each_system() {
        assert_eq!(
            both_keys("Ctrl+Shift+F"),
            ("Ctrl+Shift+F".to_owned(), "Cmd+Shift+F".to_owned())
        );
        assert_eq!(
            both_keys("Cmd+[, Cmd+]"),
            ("Alt+Left, Alt+Right".to_owned(), "Cmd+[, Cmd+]".to_owned())
        );
    }

    /// The site in `docs/` is the site that the sources make. With `GENIE_WRITE_SITE` set, the
    /// test writes it.
    #[test]
    fn the_site_is_up_to_date() {
        let docs = root().join("docs");
        let write = std::env::var_os("GENIE_WRITE_SITE").is_some();
        let mut stale = Vec::new();
        for (path, bytes) in build() {
            let target = docs.join(&path);
            if write {
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(&target, &bytes).unwrap();
            } else if fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
                stale.push(path);
            }
        }
        assert!(
            stale.is_empty(),
            "These files of docs/ are out of date: {stale:?}. Write them with GENIE_WRITE_SITE=1 cargo test -p genie --bin genie site"
        );
    }

    /// Each example session opens: it has a reference and its tracks.
    #[test]
    fn example_sessions_are_valid() {
        let folder = root().join("site").join("examples");
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            let saved = Saved::parse(&text, Path::new("/examples")).unwrap();
            let tracks = text.matches("[track]").count();
            assert_eq!(saved.tracks.len(), tracks, "{}", path.display());
            assert!(saved.locus.is_some(), "{}", path.display());
        }
    }
}
