"""Build labeled before/after sheets from effects-lab captures (requires Pillow)."""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

parser = argparse.ArgumentParser()
parser.add_argument('captures', type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
effects = sorted((json.loads(p.read_text(encoding='utf8')) for p in root.glob('*/effect.json')
                  if (args.captures / (p.parent.name + '.jpg')).exists()),
                 key=lambda m: (m['category'], m['id']))
before = Image.open(args.captures / 'before.jpg').convert('RGB').resize((320, 180), Image.Resampling.LANCZOS)
try:
    font = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 16)
    small = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 12)
except OSError:
    font = small = ImageFont.load_default()

def sheet(items, pairs, name):
    width = pairs * 648 + 16
    height = ((len(items) + pairs - 1) // pairs) * 224 + 50
    image = Image.new('RGB', (width, height), '#10151c')
    draw = ImageDraw.Draw(image)
    draw.text((16, 12), 'mypaper / ' + name + ' / before → after', font=font, fill='#e6edf5')
    for i, m in enumerate(items):
        x, y = 16 + (i % pairs) * 648, 46 + (i // pairs) * 224
        draw.text((x, y), f"{m['name']}  ·  {m['id']}", font=font, fill='#e6edf5')
        draw.text((x+390, y+3), f"{'moving' if m['animated'] else 'still'} / {m['cost']:.3f} ms", font=small, fill='#9eafc1')
        image.paste(before, (x, y+28))
        after = Image.open(args.captures / (m['id']+'.jpg')).convert('RGB').resize((320,180), Image.Resampling.LANCZOS)
        image.paste(after, (x+324, y+28))
    target = args.captures / (name+'.jpg')
    image.save(target, quality=94)
    print(target)

sheet(effects, 3, 'contact-sheet')
for category in sorted({m['category'] for m in effects}):
    sheet([m for m in effects if m['category'] == category], 2, category)
