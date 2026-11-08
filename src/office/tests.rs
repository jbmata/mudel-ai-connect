use super::*;
use std::io::{Cursor, Write};
use zip::{ZipWriter, write::SimpleFileOptions};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn archive(files: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn run(paragraph: &str) -> String {
    format!("<w:r><w:t>{paragraph}</w:t></w:r>")
}

#[test]
fn docx_keeps_paragraphs_entities_tabs_and_tables() {
    let document = format!(
        r#"<w:document xmlns:w="{W}"><w:body>
        <w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr>{}</w:p>
        <w:p><w:r><w:t>Uno</w:t><w:tab/><w:t>dos</w:t><w:br/><w:t>tres &#x1F600;</w:t></w:r></w:p>
        <w:tbl>
          <w:tr><w:tc><w:p>{}</w:p></w:tc><w:tc><w:p>{}</w:p></w:tc></w:tr>
          <w:tr><w:tc><w:p>{}</w:p></w:tc><w:tc><w:p>{}</w:p></w:tc></w:tr>
        </w:tbl>
        <w:p>{}</w:p>
        </w:body></w:document>"#,
        run("Cálculo &amp; álgebra &lt;1&gt;"),
        run("A1"),
        run("B1"),
        run("A2"),
        run("B2"),
        run("Fin"),
    );
    let body = archive(&[("word/document.xml", &document)]);
    let text = extract(Kind::Docx, &body).unwrap();
    // Una definición de tabulador (w:tabs) no es un carácter de tabulación.
    assert_eq!(
        text,
        "Cálculo & álgebra <1>\nUno\tdos\ntres 😀\nA1 | B1\nA2 | B2\nFin"
    );
}

#[test]
fn pptx_reads_slides_in_numeric_order_with_speaker_notes() {
    let slide = |text: &str| {
        format!(
            r#"<p:sld xmlns:p="x" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:txBody>
            <a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    };
    let notes = format!(
        r#"<p:notes xmlns:p="x" xmlns:a="{A}"><p:txBody><a:p><a:r><a:t>Recordar la derivada</a:t></a:r></a:p>
        <a:p><a:fld type="slidenum"><a:t>2</a:t></a:fld></a:p></p:txBody></p:notes>"#
    );
    let rels = r#"<Relationships xmlns="r"><Relationship Id="rId1" Type="http://x/slideLayout" Target="../slideLayouts/l.xml"/>
        <Relationship Id="rId2" Type="http://x/notesSlide" Target="../notesSlides/notesSlide7.xml"/></Relationships>"#;
    let empty = format!(r#"<p:sld xmlns:p="x" xmlns:a="{A}"/>"#);
    let body = archive(&[
        ("ppt/slides/slide10.xml", &slide("Diez")),
        ("ppt/slides/slide2.xml", &slide("Dos")),
        ("ppt/slides/slide3.xml", &empty),
        ("ppt/slides/_rels/slide2.xml.rels", rels),
        ("ppt/notesSlides/notesSlide7.xml", &notes),
    ]);
    let text = extract(Kind::Pptx, &body).unwrap();
    assert_eq!(
        text,
        "--- Diapositiva 2 ---\nDos\n\nNotas del ponente:\nRecordar la derivada\n\n--- Diapositiva 3 ---\n(sin texto)\n\n--- Diapositiva 10 ---\nDiez"
    );
}

#[test]
fn xlsx_resolves_shared_inline_and_boolean_cells_and_skips_hidden_sheets() {
    let shared = format!(
        r#"<sst xmlns="{S}"><si><t>Nombre</t></si><si><r><t>Nota</t></r><r><t> final</t></r></si>
        <si><t>Ana</t><rPh sb="0" eb="1"><t>fonética</t></rPh></si></sst>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{S}" xmlns:r="r"><sheets>
        <sheet name="Notas" sheetId="1" r:id="rId1"/>
        <sheet name="Oculta" sheetId="2" state="hidden" r:id="rId2"/>
        <sheet name="Otra" sheetId="3" r:id="rId3"/></sheets></workbook>"#
    );
    let rels = r#"<Relationships xmlns="r">
        <Relationship Id="rId1" Type="t" Target="worksheets/sheet1.xml"/>
        <Relationship Id="rId2" Type="t" Target="worksheets/sheet2.xml"/>
        <Relationship Id="rId3" Type="t" Target="/xl/worksheets/sheet3.xml"/></Relationships>"#;
    let sheet1 = format!(
        r#"<worksheet xmlns="{S}"><sheetData>
        <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
        <row r="2"><c r="A2" t="s"><v>2</v></c><c r="C2"><v>8.5</v></c></row>
        <row r="3"/>
        <row r="4"><c r="B4" t="b"><v>1</v></c><c r="D4" t="inlineStr"><is><t>Texto &amp; más</t></is></c></row>
        </sheetData></worksheet>"#
    );
    let sheet3 = format!(
        r#"<worksheet xmlns="{S}"><sheetData><row r="1"><c r="A1"><v>42</v></c></row></sheetData></worksheet>"#
    );
    let body = archive(&[
        ("xl/sharedStrings.xml", &shared),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", &sheet1),
        ("xl/worksheets/sheet2.xml", "<worksheet>secreto</worksheet>"),
        ("xl/worksheets/sheet3.xml", &sheet3),
    ]);
    let text = extract(Kind::Xlsx, &body).unwrap();
    assert_eq!(
        text,
        "## Hoja: Notas\nNombre\tNota final\nAna\t\t8.5\n\tVERDADERO\t\tTexto & más\n\n## Hoja: Otra\n42"
    );
    assert!(!text.contains("secreto") && !text.contains("fonética"));
}

#[test]
fn invalid_encrypted_empty_and_legacy_documents_give_clear_errors() {
    assert!(
        extract(Kind::Docx, b"no es un zip")
            .unwrap_err()
            .contains("documento de Office válido")
    );
    // Un OLE2 (los .docx con contraseña) tampoco es un ZIP.
    assert!(
        extract(
            Kind::Docx,
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0]
        )
        .is_err()
    );
    let no_document = archive(&[("otra.xml", "<a/>")]);
    assert!(extract(Kind::Docx, &no_document).is_err());
    assert!(extract(Kind::Pptx, &no_document).is_err());
    assert!(extract(Kind::Xlsx, &no_document).is_err());
    let empty = archive(&[(
        "word/document.xml",
        &format!(r#"<w:document xmlns:w="{W}"/>"#),
    )]);
    assert!(
        extract(Kind::Docx, &empty)
            .unwrap_err()
            .contains("no contiene texto")
    );
    let broken = archive(&[("word/document.xml", "<w:document><w:p></w:q>")]);
    assert!(extract(Kind::Docx, &broken).is_err());
    assert_eq!(kind("docx"), Some(Kind::Docx));
    assert_eq!(kind("doc"), None);
    assert!(legacy_hint("ppt").unwrap().contains(".pptx"));
    assert!(legacy_hint("pdf").is_none());
}

#[test]
fn a_zip_bomb_is_refused_without_exhausting_memory() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "word/document.xml",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    // 80 MiB de ceros comprimen a unos pocos KiB.
    let block = vec![b' '; 1024 * 1024];
    for _ in 0..80 {
        writer.write_all(&block).unwrap();
    }
    let body = writer.finish().unwrap().into_inner();
    assert!(body.len() < 200 * 1024);
    assert!(
        extract(Kind::Docx, &body)
            .unwrap_err()
            .contains("demasiado")
    );
}

#[test]
fn figures_are_flagged_in_the_text_of_slides_documents_and_workbooks() {
    let slide = |body: &str| {
        format!(
            r#"<p:sld xmlns:p="x" xmlns:a="{A}" xmlns:r="r"><p:cSld><p:spTree><p:sp><p:txBody>
            <a:p><a:r><a:t>Texto</a:t></a:r></a:p></p:txBody></p:sp>{body}</p:spTree></p:cSld></p:sld>"#
        )
    };
    let pptx = archive(&[
        (
            "ppt/slides/slide1.xml",
            &slide(
                r#"<p:pic><a:blip r:embed="rId2"/></p:pic><p:pic><a:blip r:embed="rId3"/></p:pic><p:cxnSp/>"#,
            ),
        ),
        ("ppt/slides/slide2.xml", &slide("")),
    ]);
    let text = extract(Kind::Pptx, &pptx).unwrap();
    assert!(
        text.contains(
            "Texto\n[figuras: 2 imágenes y dibujos o gráficos; se ven con view_course_file]"
        ),
        "{text}"
    );
    assert_eq!(text.matches("[figuras").count(), 1, "{text}");

    let document = format!(
        r#"<w:document xmlns:w="{W}" xmlns:a="{A}" xmlns:r="r"><w:body><w:p>{}</w:p><w:p><a:blip r:embed="rId1"/></w:p></w:body></w:document>"#,
        run("Hola")
    );
    let docx = extract(Kind::Docx, &archive(&[("word/document.xml", &document)])).unwrap();
    assert!(
        docx.ends_with("[figuras: 1 imagen; se ven con view_course_file]"),
        "{docx}"
    );

    let sheet = format!(
        r#"<worksheet xmlns="{S}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row></sheetData></worksheet>"#
    );
    let workbook = format!(
        r#"<workbook xmlns="{S}" xmlns:r="r"><sheets><sheet name="Datos" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let rels = r#"<Relationships xmlns="r"><Relationship Id="rId1" Type="http://x/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let xlsx = archive(&[
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", &sheet),
        ("xl/media/image1.png", "x"),
        ("xl/charts/chart1.xml", "<c/>"),
    ]);
    let text = extract(Kind::Xlsx, &xlsx).unwrap();
    assert!(
        text.contains("[figuras: 1 imágenes y 1 gráficos;"),
        "{text}"
    );
}
