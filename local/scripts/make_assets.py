"""Genera, a partir de shared/assets/logo.png, la imagen de la ventana de instalación y el icono del .exe."""

from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[2]
LOCAL = ROOT / "local"


def square_logo() -> Image.Image:
    logo = Image.open(ROOT / "shared" / "assets" / "logo.png").convert("RGB")
    background = Image.new("RGB", logo.size, (254, 254, 254))
    box = ImageChops.difference(logo, background).point(lambda value: 255 if value > 24 else 0).getbbox()
    cropped = logo.crop(box)
    side = int(max(cropped.size) * 1.08)
    canvas = Image.new("RGB", (side, side), (255, 255, 255))
    canvas.paste(cropped, ((side - cropped.width) // 2, (side - cropped.height) // 2))
    return canvas


if __name__ == "__main__":
    logo = square_logo()
    logo.resize((132, 132), Image.LANCZOS).save(LOCAL / "assets" / "logo-setup.png", optimize=True)
    logo.save(LOCAL / "assets" / "logo.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)])
    print("assets/logo-setup.png, assets/logo.ico")
