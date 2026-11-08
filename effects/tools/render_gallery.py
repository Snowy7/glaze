"""Reproduce curated effect thumbnails with the real offline WGSL renderer (Pillow)."""
import argparse
import json
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


def grid(manifests, target):
    font = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 17)
    small = ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf', 12)
    heading = ImageFont.truetype('C:/Windows/Fonts/segoeuib.ttf', 26)
    columns = 5
    image = Image.new('RGB', (columns * 336 + 16, ((len(manifests) + columns - 1) // columns) * 232 + 84), '#10151c')
    draw = ImageDraw.Draw(image)
    draw.text((16, 12), 'mypaper / effect gallery', font=heading, fill='#eef4fb')
    draw.text((16, 48), 'Actual WGSL renders / curated photos and representative frames / 320 x 180 cards', font=small, fill='#a4b5c8')
    for i, manifest in enumerate(manifests):
        x, y = 16 + i % columns * 336, 78 + i // columns * 232
        image.paste(Image.open(ROOT / 'effects' / manifest['id'] / 'preview.jpg'), (x, y))
        draw.text((x, y + 185), manifest['name'], font=font, fill='#eef4fb')
        draw.text((x, y + 209), f"{manifest['category']} / {manifest['id']}", font=small, fill='#a4b5c8')
    image.save(target, quality=95, subsampling=0)
    print(target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lab', type=Path, required=True)
    parser.add_argument('--out', type=Path, default=Path('C:/mypaper-targets/effects-sheet/gallery-v2'))
    parser.add_argument('--ffmpeg', default=shutil.which('ffmpeg'))
    parser.add_argument('--only', default='')
    parser.add_argument('--grid-only', action='store_true')
    args = parser.parse_args()
    config = json.loads((ROOT / 'effects/tools/gallery.json').read_text())
    manifests = sorted((json.loads(p.read_text()) for p in (ROOT / 'effects').glob('*/effect.json')),
                       key=lambda m: (m['category'], m['id']))
    assert set(config['effects']) == {m['id'] for m in manifests}, 'Every package needs a recipe'
    args.out.mkdir(parents=True, exist_ok=True)
    sources = {}
    for manifest in manifests:
        effect_id = manifest['id']
        if args.grid_only or (args.only and effect_id not in args.only.split(',')):
            continue
        recipe = config['effects'][effect_id]
        source = config['sources'][recipe['photo']]
        folder = args.out / effect_id
        folder.mkdir(parents=True, exist_ok=True)
        if recipe['photo'] not in sources:
            if 'video' in source:
                assert args.ffmpeg, 'Pass --ffmpeg to extract the licensed daytime frame'
                decoded = args.out / (recipe['photo'] + '.jpg')
                subprocess.run([args.ffmpeg, '-v', 'error', '-y', '-ss', str(source['frame']),
                                '-i', str(ROOT / source['video']), '-frames:v', '1', str(decoded)], check=True)
            else:
                decoded = ROOT / source['path']
            sources[recipe['photo']] = Image.open(decoded).convert('RGB')
        photo = crop(sources[recipe['photo']], recipe.get('crop'))
        photo = ImageOps.fit(photo, (1920, 1080), method=Image.Resampling.LANCZOS)
        input_path = folder / 'input.jpg'
        photo.save(input_path, quality=97, subsampling=0)
        command = [str(args.lab), '--render', '--include-reference', '--no-previews', '--only', effect_id,
                   '--photo', str(input_path), '--out', str(folder), '--params', json.dumps(recipe.get('params', {})),
                   '--clock-hours', str(recipe.get('clock_hours', 17.5))]
        if 'time' in recipe:
            command.extend(['--time', str(recipe['time'])])
        if 'mouse' in recipe:
            command.extend(['--mouse', ','.join(map(str, recipe['mouse']))])
        if recipe.get('event_peak'):
            command.append('--event-peak')
        subprocess.run(command, cwd=ROOT, check=True)
        rendered = Image.open(folder / (effect_id + '.jpg')).convert('RGB')
        # A still card can convey cursor/wind travel with two actual displaced frames.
        if 'motion' in recipe:
            first = rendered.copy()
            motion = recipe['motion']
            second_command = command.copy()
            for key in ('time', 'mouse'):
                if key not in motion:
                    continue
                flag = '--' + key
                value = ','.join(map(str, motion[key])) if key == 'mouse' else str(motion[key])
                if flag in second_command:
                    second_command[second_command.index(flag) + 1] = value
                else:
                    second_command.extend([flag, value])
            first.save(folder / 'motion-first.jpg', quality=95, subsampling=0)
            subprocess.run(second_command, cwd=ROOT, check=True)
            second = Image.open(folder / (effect_id + '.jpg')).convert('RGB')
            second.save(folder / 'motion-second.jpg', quality=95, subsampling=0)
            rendered = Image.blend(first, second, motion.get('blend', 0.38))
            rendered.save(folder / (effect_id + '.jpg'), quality=95, subsampling=0)
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
        (folder / 'recipe.json').write_text(json.dumps({'effect': effect_id, 'source': source, 'recipe': recipe}, indent=2) + '\n')
    grid(manifests, args.out.parent / 'preview-grid-v2.jpg')


if __name__ == '__main__':
    main()
