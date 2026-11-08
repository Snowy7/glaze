"""Build labeled before/after sheets from effects-lab captures (requires Pillow)."""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

parser = argparse.ArgumentParser()
parser.add_argument('captures', type=Path)
parser.add_argument('--output', type=Path, help='Master sheet path (category sheets stay beside captures)')
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
effects = sorted((json.loads(p.read_text(encoding='utf8')) for p in root.glob('*/effect.json')
                  if (args.captures / (p.parent.name + '.jpg')).exists()),
                 key=lambda m: (m['category'], m['id']))
before = Image.open(args.captures / 'before.jpg').convert('RGB').resize((320, 180), Image.Resampling.LANCZOS)
timing_path = args.captures / 'timings.json'
photo = str(json.loads(timing_path.read_text())['photo']) if timing_path.exists() else ''
if 'daytime' in photo or 'earth-orbit' in photo:
    credit = 'Earth Views Extended Cut for Earth Day 2021 (1 s) / NASA (JSC) / public domain / cropped, resized and effect-processed'
    source = 'https://images.nasa.gov/details/jsc2021m000138_4K_Earth_Views_Extended_Cut_for_Earth_Day_%202021_210422-4KMP4'
elif 'snow-forest' in photo:
    credit = 'Spring frame (105 s) / Blender Foundation, Andy Goralczyk / CC BY 4.0 / cropped, resized and effect-processed'
    source = 'https://studio.blender.org/films/spring/'
else:
    credit = 'Hong Kong Skyline Restitch - Dec 2007 / David Iliff (Diliff) / CC BY 3.0 / cropped, resized and effect-processed'
    source = 'https://commons.wikimedia.org/wiki/File:Hong_Kong_Skyline_Restitch_-_Dec_2007.jpg'
(args.captures / 'CREDITS.txt').write_text(credit+'\n'+source+'\nOriginal effect code: mypaper, MIT.\n', encoding='utf8')
try:
    font = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 16)
    small = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 12)
except OSError:
    font = small = ImageFont.load_default()

def sheet(items, pairs, name):
    width = pairs * 648 + 16
    height = ((len(items) + pairs - 1) // pairs) * 224 + 90
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
    draw.text((16, height-34), credit, font=small, fill='#9eafc1')
    draw.text((16, height-18), source, font=small, fill='#9eafc1')
    # Category names can also be effect IDs (notably color); never overwrite a capture.
    filename = 'contact-sheet.jpg' if name == 'contact-sheet' else 'category-' + name + '.jpg'
    target = args.output if name == 'contact-sheet' and args.output else args.captures / filename
    image.save(target, quality=94)
    print(target)

sheet(effects, 3, 'contact-sheet')
for category in sorted({m['category'] for m in effects}):
    sheet([m for m in effects if m['category'] == category], 2, category)
