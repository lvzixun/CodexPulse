"""Generate the Pulse vector mark and platform icons; requires Pillow (dev only)."""

from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
ICONS = ROOT / "apps/desktop/src-tauri/icons"
ASSETS = ROOT / "apps/desktop/src/lib/assets"
POINTS = [(20, 54), (35, 54), (43, 30), (56, 73), (65, 47), (80, 47)]
START = (88, 185, 255)
END = (46, 81, 205)
STROKE = 6.5


def tile(size):
    """Render from the same geometry and colors as the UI's vector asset."""
    image = Image.new("RGBA", (size, size))
    pixels = image.load()
    for y in range(size):
        for x in range(size):
            amount = (x + y) / (2 * (size - 1))
            pixels[x, y] = tuple(round(a + (b - a) * amount) for a, b in zip(START, END)) + (255,)
    mask = Image.new("L", image.size)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size - 1, size - 1), radius=size * .225, fill=255)
    image.putalpha(mask)
    draw = ImageDraw.Draw(image)
    points = [(x * size / 100, y * size / 100) for x, y in POINTS]
    width = round(size * STROKE / 100)
    draw.line(points, fill="white", width=width, joint="curve")
    for x, y in points:
        radius = width / 2
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill="white")
    return image


def app_icon(size, inset, shadow=False):
    # Supersampling keeps the small Windows tray/taskbar sizes crisp.
    scale = 2 if size >= 512 else 4
    canvas = Image.new("RGBA", (size * scale, size * scale))
    padding = round(size * scale * inset)
    body = tile(size * scale - padding * 2)
    if shadow:
        silhouette = Image.new("RGBA", canvas.size)
        silhouette.paste((0, 0, 0, 40), (padding, padding + 8 * scale), body.getchannel("A"))
        canvas = silhouette.filter(ImageFilter.GaussianBlur(12 * scale))
    canvas.alpha_composite(body, (padding, padding))
    return canvas.resize((size, size), Image.Resampling.LANCZOS)


ICONS.mkdir(parents=True, exist_ok=True)
ASSETS.mkdir(parents=True, exist_ok=True)
path = "M" + " L".join(f"{x} {y}" for x, y in POINTS)
svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">
  <defs><linearGradient id="pulse" x1="0" y1="0" x2="100" y2="100" gradientUnits="userSpaceOnUse"><stop stop-color="#{bytes(START).hex()}"/><stop offset="1" stop-color="#{bytes(END).hex()}"/></linearGradient></defs>
  <rect width="100" height="100" rx="22.5" fill="url(#pulse)"/>
  <path d="{path}" stroke="white" stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
'''
(ASSETS / "app-icon.svg").write_text(svg)
mac = app_icon(1024, .094, shadow=True)
mac.save(ICONS / "icon.png")
mac.save(ICONS / "icon.icns")
windows = app_icon(1024, .035)
windows.save(ICONS / "icon.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
for size in (32, 128, 256):
    app_icon(size, .035).save(ICONS / f"{size}x{size}.png")
