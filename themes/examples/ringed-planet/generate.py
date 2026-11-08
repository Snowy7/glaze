"""Original mypaper CC0 planet fixture; rebuild with Python + Pillow."""
import io,json,math,struct
from pathlib import Path
from PIL import Image
blob=bytearray(); views=[]; accessors=[]; images=[]; textures=[]
def data(raw):
    blob.extend(b'\0'*(-len(blob)%4)); views.append(dict(buffer=0,byteOffset=len(blob),byteLength=len(raw))); blob.extend(raw); return len(views)-1
def acc(rows,kind,component=5126,bounds=False):
    flat=rows if kind=='SCALAR' else [v for r in rows for v in r]
    a=dict(bufferView=data(struct.pack('<'+('f' if component==5126 else 'I')*len(flat),*flat)),componentType=component,count=len(rows),type=kind)
    if bounds:a.update(min=[min(r[i] for r in rows) for i in range(len(rows[0]))],max=[max(r[i] for r in rows) for i in range(len(rows[0]))])
    accessors.append(a); return len(accessors)-1
def tex(im):
    stream=io.BytesIO(); im.save(stream,'PNG'); images.append(dict(bufferView=data(stream.getvalue()),mimeType='image/png')); textures.append(dict(source=len(images)-1,sampler=0)); return dict(index=len(textures)-1)
base=Image.new('RGBA',(256,128)); normal=Image.new('RGBA',base.size); mr=Image.new('RGBA',base.size); emissive=Image.new('RGBA',base.size)
for y in range(128):
    for x in range(256):
        u,v=x/256,y/128; phase=v*85+math.sin(u*math.tau*3)*.7+math.sin(u*math.tau*8+v*10)*.22
        band=(math.sin(phase)+math.sin(phase*.43)*.6)/1.6; blue=.5+band*.5
        base.putpixel((x,y),(int(65+blue*100),int(115+blue*90),int(150+blue*80),255)); normal.putpixel((x,y),(128,int(128+math.cos(phase)*15),253,255)); mr.putpixel((x,y),(255,int(155+35*blue),40,255)); glow=max(0,band-.7)*.15; emissive.putpixel((x,y),(int(glow*40),int(glow*160),int(glow*240),255))
ring=Image.new('RGBA',(16,256))
for y in range(256):
    r=y/255; alpha=(.5+.5*math.sin(r*190))*.35+.45
    if .46<r<.51 or r<.015 or r>.985:alpha=0
    for x in range(16):ring.putpixel((x,y),(220,178+int(r*35),134+int(r*55),int(alpha*255)))
materials=[dict(name='Icy banded world',pbrMetallicRoughness=dict(baseColorTexture=tex(base),metallicRoughnessTexture=tex(mr),metallicFactor=.6,roughnessFactor=.85),normalTexture=tex(normal),emissiveTexture=tex(emissive),emissiveFactor=[.15,.3,.5]),dict(name='Translucent ice rings',pbrMetallicRoughness=dict(baseColorTexture=tex(ring),metallicFactor=.4,roughnessFactor=.48),alphaMode='BLEND',doubleSided=True)]
pos=[]; normals=[]; uv=[]; indices=[]; morph=[]; segments=64; rows=32
for y in range(rows+1):
    theta=y/rows*math.pi
    for x in range(segments+1):
        phi=x/segments*math.tau; p=[math.sin(theta)*math.cos(phi),math.cos(theta),math.sin(theta)*math.sin(phi)]; pos.append(p); normals.append(p); uv.append([x/segments,y/rows]); morph.append([p[0]*.08,-p[1]*.08,p[2]*.08])
for y in range(rows):
    for x in range(segments):
        a=y*(segments+1)+x; b=a+segments+1; indices.extend([a,a+1,b,a+1,b+1,b])
sphere=dict(attributes=dict(POSITION=acc(pos,'VEC3',bounds=True),NORMAL=acc(normals,'VEC3'),TEXCOORD_0=acc(uv,'VEC2')),indices=acc(indices,'SCALAR',5125),material=0,targets=[dict(POSITION=acc(morph,'VEC3',bounds=True))])
pos=[]; normals=[]; uv=[]; indices=[]
for x in range(129):
    phi=x/128*math.tau
    for edge,r in enumerate([1.35,2.1]):pos.append([math.cos(phi)*r,0,math.sin(phi)*r]); normals.append([0,1,0]); uv.append([x/128,edge])
for x in range(128):
    a=x*2; indices.extend([a,a+2,a+1,a+1,a+2,a+3])
rings=dict(attributes=dict(POSITION=acc(pos,'VEC3',bounds=True),NORMAL=acc(normals,'VEC3'),TEXCOORD_0=acc(uv,'VEC2')),indices=acc(indices,'SCALAR',5125),material=1)
times=acc([0,2,4,6,8],'SCALAR'); accessors[times].update(min=[0],max=[8]); rotations=acc([[0,math.sin(t/8*math.pi),0,math.cos(t/8*math.pi)] for t in [0,2,4,6,8]],'VEC4'); weights=acc([0,1,0,1,0],'SCALAR')
doc=dict(asset=dict(version='2.0',generator='mypaper CC0 ringed planet'),scene=0,scenes=[dict(nodes=[0,1])],nodes=[dict(name='Planet',mesh=0),dict(name='Rings',mesh=1,rotation=[0,0,math.sin(.22),math.cos(.22)])],meshes=[dict(primitives=[sphere],weights=[0]),dict(primitives=[rings])],materials=materials,textures=textures,images=images,samplers=[dict(magFilter=9729,minFilter=9987,wrapS=10497,wrapT=33071)],animations=[dict(name='Turntable',samplers=[dict(input=times,output=rotations)],channels=[dict(sampler=0,target=dict(node=0,path='rotation'))]),dict(name='Breathe',samplers=[dict(input=times,output=weights)],channels=[dict(sampler=0,target=dict(node=0,path='weights'))])],accessors=accessors,bufferViews=views,buffers=[dict(byteLength=len(blob))])
js=json.dumps(doc,separators=(',',':')).encode(); js+=b' '*(-len(js)%4); blob.extend(b'\0'*(-len(blob)%4)); body=struct.pack('<I4s',len(js),b'JSON')+js+struct.pack('<I4s',len(blob),b'BIN\0')+blob
Path(__file__).with_name('planet.glb').write_bytes(struct.pack('<4sII',b'glTF',2,len(body)+12)+body)
print(f'Wrote {len(body)+12:,} bytes')
