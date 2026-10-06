"""Generate the geometric Pulse app icon; requires Pillow (development only)."""
from pathlib import Path
from PIL import Image, ImageDraw

directory = Path(__file__).resolve().parents[1] / 'apps/desktop/src-tauri/icons'
directory.mkdir(parents=True, exist_ok=True)
image = Image.new('RGBA', (512, 512), (0, 0, 0, 0))
draw = ImageDraw.Draw(image)
draw.rounded_rectangle((12, 12, 500, 500), radius=112, fill='#1e2036')
draw.line([(82, 271), (175, 271), (224, 139), (291, 372), (345, 271), (430, 271)], fill='#8bbbff', width=30, joint='curve')
for point in [(82, 271), (430, 271)]:
    draw.ellipse((point[0]-15, point[1]-15, point[0]+15, point[1]+15), fill='#8bbbff')
image.save(directory/'icon.png')
image.save(directory/'icon.ico', sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
for size in [32,128,256]:
    image.resize((size,size),Image.Resampling.LANCZOS).save(directory/f'{size}x{size}.png')
