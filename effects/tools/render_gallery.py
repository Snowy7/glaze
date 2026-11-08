"""Render the curated effect thumbnails through the engine's real stack path (Pillow).

Each card's input photo and job go to <out>/<id>/, then one engine test run
(`gallery_renders` in crates/wallpaper/src/sources/stack/effects_real.rs) renders them all with
the same StackSource that Studio's preview and the desktop use, so preview.jpg is what users see.
"""
import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont, ImageOps

ROOT = Path(__file__).resolve().parents[2]
REFERENCES = {'rain', 'blur', 'color', 'vignette', 'grain'}


def crop(image, box):
    if box is None:
        return image
    left, top, right, bottom = box
    assert 0 <= left < right <= 1 and 0 <= top < bottom <= 1, box
    return image.crop((round(left * image.width), round(top * image.height),
                       round(right * image.width), round(bottom * image.height)))


def hash21(x, y):
    """The prelude's hash21, to land a capture inside a lightning or glitch event."""
    x, y = (x * 123.34) % 1.0, (y * 456.21) % 1.0
    d = x * (x + 45.32) + y * (y + 45.32)
    return ((x + d) * (y + d)) % 1.0


def capture_time(manifest, recipe):
    """Seconds to capture: during an event for the rare-event effects, else 12 s in."""
    if 'time' in recipe:
        return recipe['time']
    defaults = {p['id']: p.get('default') for p in manifest['params']}
    freq = recipe.get('params', {}).get('frequency', defaults.get('frequency', 4.0))
    if manifest['id'] == 'lightning':
        return hash21(0.0, 17.0) * 0.5 * 60.0 / min(max(freq, 0.5), 15.0) + 0.025
    if manifest['id'] == 'glitch':
        return hash21(0.0, 31.0) * 0.65 * 60.0 / min(max(freq, 0.5), 20.0) + 0.02
    return 12.0


def grid(manifests, target):
    font = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 17)
    small = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 12)
    heading = ImageFont.truetype('C:/Windows/Fonts/segoeuib.ttf', 26)
    columns = 5
    image = Image.new('RGB', (columns * 336 + 16, ((len(manifests) + columns - 1) // columns) * 232 + 84), '#10151c')
    draw = ImageDraw.Draw(image)
    draw.text((16, 12), 'Glaze / effect gallery', font=heading, fill='#eef4fb')
    draw.text((16, 48), 'Engine renders (the real stack path) / curated photos and representative frames / 320 x 180 cards',
              font=small, fill='#a4b5c8')
    for i, manifest in enumerate(manifests):
        x, y = 16 + i % columns * 336, 78 + i // columns * 232
        image.paste(Image.open(ROOT / 'effects' / manifest['id'] / 'preview.jpg'), (x, y))
        draw.text((x, y + 185), manifest['name'], font=font, fill='#eef4fb')
        draw.text((x, y + 209), f"{manifest['category']} / {manifest['id']}", font=small, fill='#a4b5c8')
    image.save(target, quality=95, subsampling=0)
    print(target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=Path('C:/glaze-targets/effects-sheet/gallery-v3'))
    parser.add_argument('--target-dir', default=os.environ.get('CARGO_TARGET_DIR', 'C:/glaze-targets/main'))
    parser.add_argument('--ffmpeg', default=shutil.which('ffmpeg'))
    # Video frames are extracted once and reused (ffmpeg is only needed the first time).
    parser.add_argument('--frames', type=Path, default=Path('C:/glaze-targets/effects-sheet/gallery-v2'))
    parser.add_argument('--only', default='')
    parser.add_argument('--grid-only', action='store_true')
    args = parser.parse_args()
    config = json.loads((ROOT / 'effects/tools/gallery.json').read_text())
    manifests = sorted((json.loads(p.read_text()) for p in (ROOT / 'effects').glob('*/effect.json')),
                       key=lambda m: (m['category'], m['id']))
    assert set(config['effects']) == {m['id'] for m in manifests}, 'Every package needs a recipe'
    chosen = [m for m in manifests
              if not args.grid_only and (not args.only or m['id'] in args.only.split(','))]
    if chosen and args.out.exists():
        shutil.rmtree(args.out)
    args.out.mkdir(parents=True, exist_ok=True)
    sources = {}
    for manifest in chosen:
        effect_id = manifest['id']
        recipe = config['effects'][effect_id]
        source = config['sources'][recipe['photo']]
        folder = args.out / effect_id
        folder.mkdir(parents=True, exist_ok=True)
        if recipe['photo'] not in sources:
            if 'video' in source:
                decoded = args.frames / (recipe['photo'] + '.jpg')
            if 'video' in source and not decoded.exists():
                assert args.ffmpeg, 'Pass --ffmpeg to extract the licensed daytime frame'
                args.frames.mkdir(parents=True, exist_ok=True)
                subprocess.run([args.ffmpeg, '-v', 'error', '-y', '-ss', str(source['frame']),
                                '-i', str(ROOT / source['video']), '-frames:v', '1', str(decoded)], check=True)
            elif 'video' not in source:
                decoded = ROOT / source['path']
            sources[recipe['photo']] = Image.open(decoded).convert('RGB')
        photo = ImageOps.fit(crop(sources[recipe['photo']], recipe.get('crop')), (1920, 1080),
                             method=Image.Resampling.LANCZOS)
        photo.save(folder / 'input.jpg', quality=97, subsampling=0)
        job = {'effect': effect_id, 'params': recipe.get('params', {}),
               'time': capture_time(manifest, recipe), 'mouse': recipe.get('mouse', [0.63, 0.42]),
               'clock_hours': recipe.get('clock_hours', 17.5)}
        if 'motion' in recipe:
            job['second'] = {'time': recipe['motion'].get('time', job['time']),
                             'mouse': recipe['motion'].get('mouse', job['mouse'])}
        (folder / 'job.json').write_text(json.dumps(job, indent=2) + '\n')
    if chosen:
        env = dict(os.environ, GLAZE_GALLERY=str(args.out), CARGO_TARGET_DIR=args.target_dir)
        subprocess.run(['cargo', 'test', '-p', 'glaze-wallpaper', '--bins', 'gallery_renders', '--',
                        '--ignored', '--test-threads=1', '--nocapture'], cwd=ROOT, env=env, check=True)
    for manifest in chosen:
        effect_id = manifest['id']
        recipe = config['effects'][effect_id]
        source = config['sources'][recipe['photo']]
        folder = args.out / effect_id
        rendered = Image.open(folder / (effect_id + '.jpg')).convert('RGB')
        # A still card can convey cursor/wind travel with two actual displaced frames.
        if 'motion' in recipe:
            second = Image.open(folder / (effect_id + '-second.jpg')).convert('RGB')
            rendered = Image.blend(rendered, second, recipe['motion'].get('blend', 0.38))
        thumbnail = ImageOps.fit(crop(rendered, recipe.get('detail')), (320, 180), method=Image.Resampling.LANCZOS)
        package = ROOT / 'effects' / effect_id
        thumbnail.save(package / 'preview.jpg', quality=95, subsampling=0)
        if effect_id not in REFERENCES or effect_id == 'grain':
            credits_path = package / 'CREDITS.txt'
            software = credits_path.read_text().split('Preview photograph: separate license')[0]
            credits_path.write_text(software + 'Preview photograph: separate license\n'
                + f"preview.jpg is adapted from \"{source['title']}\"\nby {source['author']}, {source['license']}.\n"
                + f"Source: {source['url']}\nLicense: {source['license_url']}\n"
                + 'Changes: cropped, resized to 320 x 180 and processed with this effect.\n'
                + 'Recipe (crop, frame and any showcase settings): ../tools/gallery.json.\n'
                + "The photo's separate attribution and license must be retained with the preview.\n")
    grid(manifests, args.out.parent / 'preview-grid-v3.jpg')


if __name__ == '__main__':
    main()
