use super::*;

/// PDF mínimo de una página con el contenido y los recursos indicados. `extra` son objetos
/// adicionales (desde el 5) ya completos, por ejemplo una imagen.
fn document(content: &str, resources: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R /Resources << {resources} >> >>"
        )
        .into_bytes(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
    ];
    objects.extend(extra.iter().cloned());
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend(object);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn image_object(side: u32) -> Vec<u8> {
    let pixels = vec![128u8; (side * side * 3) as usize];
    let mut object = format!(
        "<< /Type /XObject /Subtype /Image /Width {side} /Height {side} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n",
        pixels.len()
    )
    .into_bytes();
    object.extend(pixels);
    object.extend(b"\nendstream");
    object
}

#[test]
fn a_page_of_diagonal_and_curved_strokes_counts_as_a_drawing() {
    let mut content = String::new();
    for step in 0..20 {
        let offset = step * 10;
        content.push_str(&format!(
            "{offset} 0 m {} 50 l S 0 {offset} m 30 {} 60 {} 90 {} c S\n",
            offset + 30,
            offset + 20,
            offset + 40,
            offset + 60
        ));
    }
    let summary = pdf(&document(&content, "", &[])).expect("debe detectar el dibujo");
    assert_eq!(summary["pages"][0]["page"], 1);
    assert_eq!(summary["pages"][0]["drawings"], true);
    assert_eq!(summary["pages"][0]["images"], 0);
    assert!(
        summary["note"]
            .as_str()
            .unwrap()
            .contains("view_course_file")
    );
}

#[test]
fn tables_and_rules_made_of_straight_horizontal_and_vertical_lines_are_not_figures() {
    let mut content = String::new();
    for row in 0..40 {
        content.push_str(&format!(
            "10 {} 300 1 re S {} 10 1 300 re S 10 {} m 310 {} l S\n",
            row * 8,
            row * 8,
            row * 8,
            row * 8
        ));
    }
    assert!(pdf(&document(&content, "", &[])).is_none());
    // El PDF de texto de las pruebas tampoco tiene figuras.
    assert!(pdf(include_bytes!("../../tests/fixtures/text.pdf")).is_none());
}

#[test]
fn an_embedded_image_is_counted_but_a_tiny_one_is_not() {
    let big = document(
        "q 200 0 0 200 10 10 cm /Im0 Do Q",
        "/XObject << /Im0 5 0 R >>",
        &[image_object(64)],
    );
    let summary = pdf(&big).expect("debe detectar la imagen");
    assert_eq!(summary["pages"][0]["images"], 1);
    assert_eq!(summary["pages"][0]["drawings"], false);
    let tiny = document(
        "q 20 0 0 20 10 10 cm /Im0 Do Q",
        "/XObject << /Im0 5 0 R >>",
        &[image_object(16)],
    );
    assert!(pdf(&tiny).is_none());
}

#[test]
fn broken_or_empty_documents_give_no_summary_and_never_panic() {
    assert!(pdf(b"").is_none());
    assert!(pdf(b"%PDF-1.4 esto no es un pdf").is_none());
    assert!(pdf(include_bytes!("../../tests/fixtures/blank.pdf")).is_none());
}
