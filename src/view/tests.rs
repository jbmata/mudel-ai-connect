use super::*;
use std::io::Write;
use zip::{ZipWriter, write::SimpleFileOptions};

const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

fn png(width: u32, height: u32, mut pixel: impl FnMut(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut image = RgbImage::new(width, height);
    for (x, y, out) in image.enumerate_pixels_mut() {
        *out = image::Rgb(pixel(x, y));
    }
    let mut bytes = Vec::new();
    DynamicImage::ImageRgb8(image)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

fn noise(side: u32, mut seed: u32) -> Vec<u8> {
    png(side, side, |_, _| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let [a, b, c, _] = seed.to_be_bytes();
        [a, b, c]
    })
}

fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn size_of(shown: &Shown) -> (u32, u32) {
    image::load_from_memory(&shown.data).unwrap().dimensions()
}

fn rels(entries: &[(&str, &str)]) -> String {
    let items: String = entries
        .iter()
        .map(|(id, target)| {
            format!(r#"<Relationship Id="{id}" Type="{IMAGE}" Target="{target}"/>"#)
        })
        .collect();
    format!(r#"<Relationships xmlns="r">{items}</Relationships>"#)
}

#[test]
fn a_pdf_page_is_drawn_as_a_png_that_fits_the_limits() {
    let body = include_bytes!("../../tests/fixtures/text.pdf").to_vec();
    let rendered = pdf_pages(body, &[1]).unwrap();
    assert_eq!(rendered.total, 1);
    assert_eq!(rendered.images.len(), 1);
    let page = &rendered.images[0];
    assert_eq!(page.label, "Página 1");
    assert_eq!(page.mime, "image/png");
    assert!(page.data.starts_with(b"\x89PNG"));
    // 200 pt de lado, ampliado hasta el máximo de 4x.
    assert_eq!(size_of(page), (800, 800));
    assert!(page.data.len() <= MAX_IMAGE_BYTES);
}

#[test]
fn pdf_pages_outside_the_document_and_broken_or_encrypted_pdfs_are_refused() {
    let body = include_bytes!("../../tests/fixtures/text.pdf").to_vec();
    let error = pdf_pages(body, &[1, 2]).err().unwrap();
    assert!(error.contains("La página 2 no existe"), "{error}");
    assert!(error.contains("tiene 1 páginas"), "{error}");
    assert!(pdf_pages(b"%PDF-1.4 esto no es un pdf".to_vec(), &[1]).is_err());
    assert!(pdf_pages(Vec::new(), &[1]).is_err());
    let encrypted = include_bytes!("../../tests/fixtures/encrypted.pdf").to_vec();
    assert!(pdf_pages(encrypted, &[1]).is_err());
    // Una página en blanco se dibuja igualmente.
    let blank = include_bytes!("../../tests/fixtures/blank.pdf").to_vec();
    assert_eq!(pdf_pages(blank, &[1]).unwrap().images.len(), 1);
}

#[test]
fn big_images_are_shrunk_and_noisy_ones_fall_back_to_jpeg_within_the_size_limit() {
    let gradient = png(3000, 2000, |x, y| [(x / 12) as u8, (y / 8) as u8, 90]);
    let rendered = single_image(&gradient, "png").unwrap();
    let shown = &rendered.images[0];
    let (width, height) = size_of(shown);
    assert_eq!(width.max(height), MAX_SIDE);
    assert!(shown.data.len() <= MAX_IMAGE_BYTES);

    // Ruido: un PNG pesaría varios MB, así que se recodifica o se reduce.
    let noisy = noise(1600, 12345);
    assert!(noisy.len() > MAX_IMAGE_BYTES);
    let shown = &single_image(&noisy, "png").unwrap().images[0];
    assert!(shown.data.len() <= MAX_IMAGE_BYTES);
    assert_eq!(shown.mime, "image/jpeg");
    assert!(shown.data.starts_with(&[0xFF, 0xD8]));
}

#[test]
fn files_that_are_not_images_are_rejected_without_panicking() {
    assert!(single_image(b"no soy una imagen", "png").is_err());
    assert!(single_image(&[], "jpg").is_err());
    // Cabecera PNG válida que declara un tamaño absurdo.
    let mut bomb = png(2, 2, |_, _| [0, 0, 0]);
    bomb[16..20].copy_from_slice(&60_000u32.to_be_bytes());
    bomb[20..24].copy_from_slice(&60_000u32.to_be_bytes());
    assert!(single_image(&bomb, "png").is_err());
}

#[test]
fn transparent_pixels_become_white_when_flattened() {
    let mut image = RgbaImage::new(2, 1);
    image.put_pixel(0, 0, image::Rgba([10, 20, 30, 0]));
    image.put_pixel(1, 0, image::Rgba([10, 20, 30, 255]));
    let flat = flatten(&DynamicImage::ImageRgba8(image));
    assert_eq!(flat.get_pixel(0, 0).0, [255, 255, 255]);
    assert_eq!(flat.get_pixel(1, 0).0, [10, 20, 30]);
}

#[test]
fn a_pptx_slide_gives_its_images_and_says_what_it_cannot_show() {
    let slide = r#"<p:sld xmlns:p="p" xmlns:a="a" xmlns:r="r"><p:cSld><p:spTree>
        <p:pic><p:blipFill><a:blip r:embed="rId2"/></p:blipFill></p:pic>
        <p:pic><p:blipFill><a:blip r:embed="rId3"/></p:blipFill></p:pic>
        <p:pic><p:blipFill><a:blip r:embed="rId4"/></p:blipFill></p:pic>
        <p:pic><p:blipFill><a:blip r:embed="rId2"/></p:blipFill></p:pic>
        <p:cxnSp/><p:cxnSp/>
        </p:spTree></p:cSld></p:sld>"#;
    let other = r#"<p:sld xmlns:p="p"><p:cSld><p:spTree/></p:cSld></p:sld>"#;
    let circuit = png(300, 200, |x, y| [(x % 255) as u8, (y % 255) as u8, 0]);
    let logo = png(20, 20, |_, _| [200, 0, 0]);
    let slide_rels = rels(&[
        ("rId2", "../media/image1.png"),
        ("rId3", "../media/image2.emf"),
        ("rId4", "../media/image3.png"),
    ]);
    let body = pack(&[
        ("ppt/slides/slide1.xml", slide.as_bytes()),
        ("ppt/slides/slide2.xml", other.as_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
        ("ppt/media/image1.png", &circuit),
        ("ppt/media/image2.emf", b"EMF"),
        ("ppt/media/image3.png", &logo),
    ]);
    let rendered = office_pictures(Kind::Pptx, &body, &[1]).unwrap();
    assert_eq!(rendered.total, 2);
    assert_eq!(rendered.images.len(), 1);
    assert_eq!(rendered.images[0].label, "Diapositiva 1, imagen 1");
    assert_eq!(size_of(&rendered.images[0]), (300, 200));
    let notes = rendered.notes.join(" | ");
    assert!(notes.contains("1 imágenes están en un formato"), "{notes}");
    assert!(notes.contains("gráficos o dibujos"), "{notes}");
    assert!(notes.contains("1 imágenes muy pequeñas"), "{notes}");

    let empty = office_pictures(Kind::Pptx, &body, &[2]).unwrap();
    assert!(empty.images.is_empty());
    assert!(empty.notes[0].contains("no tienen imágenes"));

    let error = office_pictures(Kind::Pptx, &body, &[9]).err().unwrap();
    assert!(error.contains("La diapositiva 9 no existe"), "{error}");
}

#[test]
fn docx_and_xlsx_images_are_numbered_in_order() {
    let first = png(120, 90, |_, _| [0, 0, 255]);
    let second = png(100, 100, |_, _| [0, 255, 0]);
    let document = r#"<w:document xmlns:w="w" xmlns:a="a" xmlns:r="r"><w:body>
        <w:p><a:blip r:embed="rId9"/></w:p><w:p><a:blip r:embed="rId5"/></w:p></w:body></w:document>"#;
    let document_rels = rels(&[("rId5", "media/b.png"), ("rId9", "media/a.png")]);
    let docx = pack(&[
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", document_rels.as_bytes()),
        ("word/media/a.png", &first),
        ("word/media/b.png", &second),
    ]);
    let rendered = office_pictures(Kind::Docx, &docx, &[2, 1]).unwrap();
    assert_eq!(rendered.total, 2);
    let labels: Vec<_> = rendered.images.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Imagen 2", "Imagen 1"]);
    // El orden es el del documento: la primera imagen citada es a.png (120x90).
    assert_eq!(size_of(&rendered.images[1]), (120, 90));
    let error = office_pictures(Kind::Docx, &docx, &[3]).err().unwrap();
    assert!(error.contains("tiene 2 imágenes"), "{error}");

    let xlsx = pack(&[
        ("xl/workbook.xml", b"<workbook/>"),
        ("xl/media/image2.png", &second),
        ("xl/media/image1.png", &first),
        ("xl/charts/chart1.xml", b"<c/>"),
    ]);
    let rendered = office_pictures(Kind::Xlsx, &xlsx, &[1]).unwrap();
    assert_eq!(size_of(&rendered.images[0]), (120, 90));
    assert!(rendered.notes.join(" ").contains("gráficos"));
}

#[test]
fn broken_office_files_fail_cleanly() {
    assert!(office_pictures(Kind::Pptx, b"no es un zip", &[1]).is_err());
    assert!(office_pictures(Kind::Docx, &pack(&[("x", b"y")]), &[1]).is_err());
}

#[test]
fn requested_pages_are_validated_and_deduplicated() {
    assert_eq!(parts(&[3, 1, 3]).unwrap(), [3, 1]);
    assert!(parts(&[]).is_err());
    assert!(parts(&[0]).is_err());
    assert!(parts(&[1, 2, 3, 4, 5]).is_err());
}

#[test]
fn the_response_never_exceeds_the_total_byte_budget() {
    let mut budget = Budget::new();
    for index in 0..4 {
        let noisy = noise(1200, 7 + index);
        budget
            .add(
                format!("Página {index}"),
                image::load_from_memory(&noisy).unwrap(),
                true,
            )
            .unwrap();
    }
    assert!(budget.bytes <= MAX_TOTAL_BYTES);
    let rendered = budget.finish(4);
    assert!(rendered.images.iter().map(|i| i.data.len()).sum::<usize>() <= MAX_TOTAL_BYTES);
    if rendered.images.len() < 4 {
        assert!(rendered.notes[0].contains("Se omitieron por tamaño"));
    }
}

fn download(body: &[u8], content_type: &str) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn listing(base: &str, name: &str, size: usize) -> Value {
    json!([{"id": 1, "modules": [{"id": 90, "url": format!("{base}/mod/resource/view.php?id=90"), "contents": [{
        "type": "file", "filename": name, "filesize": size,
        "fileurl": format!("{base}/webservice/pluginfile.php/42/content/test?token=old&forcedownload=1")
    }]}]}])
}

fn request(pages: Vec<u32>) -> ViewFile {
    ViewFile {
        course_id: 7,
        module_id: 90,
        file_index: 0,
        pages,
    }
}

#[tokio::test]
async fn viewing_a_course_pdf_goes_through_the_download_and_drawing_path() {
    use crate::moodle::tests::{mock_with, response};
    let body = include_bytes!("../../tests/fixtures/text.pdf");
    let (client, _requests, handle) = mock_with(|base| {
        vec![
            response(200, &listing(base, "circuito.pdf", body.len()).to_string()).into_bytes(),
            download(body, "application/pdf"),
        ]
    });
    let views = view(&client, request(vec![1])).await.unwrap();
    assert_eq!(views.images.len(), 1);
    assert_eq!(views.info["filename"], "circuito.pdf");
    assert_eq!(views.info["unit"], "página");
    assert_eq!(views.info["total"], 1);
    assert_eq!(views.info["shown"], json!(["Página 1"]));
    // El resumen no deja pasar el token de la URL de descarga.
    assert!(!views.info.to_string().contains("token=old"));
    handle.join().unwrap();
}

#[tokio::test]
async fn unsupported_files_and_bad_requests_never_download() {
    use crate::moodle::tests::{mock_with, response};
    for name in ["notas.txt", "viejo.ppt"] {
        let (client, requests, handle) = mock_with(|base| {
            vec![response(200, &listing(base, name, 10).to_string()).into_bytes()]
        });
        assert!(view(&client, request(vec![1])).await.is_err(), "{name}");
        // Solo se pidió el listado del curso, nunca el archivo.
        assert_eq!(requests.try_iter().count(), 1);
        handle.join().unwrap();
    }
    // Una petición mal formada se rechaza antes de hablar con Moodle.
    let (client, _requests, _handle) = mock_with(|_| Vec::new());
    for pages in [vec![0], vec![], vec![1, 2, 3, 4, 5]] {
        assert!(view(&client, request(pages)).await.is_err());
    }
}
