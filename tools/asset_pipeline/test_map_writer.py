import io
import struct
import tempfile
import unittest
import zlib
from pathlib import Path
from types import SimpleNamespace

import numpy as np
from PIL import Image
from .map_writer import SpawnSelector, write_textures


class MapWriterTests(unittest.TestCase):
    def test_parallel_textures_match_serial_format_exactly(self):
        with tempfile.TemporaryDirectory() as work:
            root=Path(work);textures={};expected=io.BytesIO()
            rng=np.random.default_rng(23)
            for name, cube, png in [('z-cube', True, False), ('a-rgba', False, False), ('m-png', False, True), ('b-flat', False, False)]:
                pixels=rng.integers(0,256,(24 if cube else 4,4,4),dtype=np.uint8)
                if name=='b-flat':pixels[:]=0
                path=root/(name+('.png' if png else '.rgba'))
                if png:Image.fromarray(pixels).save(path)
                else:path.write_bytes(pixels.tobytes())
                textures[name]=dict(width=4,height=len(pixels),cube_faces=6 if cube else 1,
                                    **{'png' if png else 'rgba':path.name})
            for name,entry in sorted(textures.items()):
                if 'png' in entry:
                    with Image.open(root/entry['png']) as image:pixels=np.array(image.convert('RGBA'))
                else:pixels=np.frombuffer((root/entry['rgba']).read_bytes(),dtype=np.uint8).reshape(entry['height'],4,4)
                raw=(pixels if entry['cube_faces']==6 else pixels[::-1]).tobytes()
                packed=zlib.compress(raw,1);method=1
                if len(packed)>=len(raw):packed=raw;method=0
                encoded=name.encode()
                expected.write(struct.pack('<I',len(encoded))+encoded)
                expected.write(struct.pack('<5I',4,entry['height'],1,method,len(packed))+packed)
            actual=io.BytesIO();write_textures(actual,root,textures)
            self.assertEqual(actual.getvalue(),expected.getvalue())
            empty=io.BytesIO();write_textures(empty,root,{})
            self.assertEqual(empty.getvalue(),b'')

    def test_texture_failure_is_propagated(self):
        with tempfile.TemporaryDirectory() as work:
            with self.assertRaises(FileNotFoundError):
                write_textures(io.BytesIO(),Path(work),{'missing':dict(rgba='missing',width=4,height=4)})

    def test_spawn_streaming_preserves_ties_and_university_height(self):
        def mesh(x,y,z):
            return SimpleNamespace(bounds_min=(x-10,y,z-10),bounds_max=(x+10,y,z+10),triangles=[
                SimpleNamespace(a=(x-10,y,z-10),b=(x,y,z+10),c=(x+10,y,z-10))])
        selector=SpawnSelector('DIST_Test')
        selector.consider([mesh(0,5,0),mesh(0,20,0)])
        self.assertEqual(selector.result('test'),(0.,6.,-10/3))
        selector.consider([mesh(500,30,500)])
        self.assertEqual(selector.result('test'),(0.,6.,-10/3))
        university=SpawnSelector('DIST_University')
        university.consider([mesh(330,100,-710),mesh(330,132,-710),mesh(0,132,0)])
        self.assertEqual(university.result('University'),(330.,133.,-710.))
        with self.assertRaisesRegex(ValueError,'No supported spawn'):
            SpawnSelector('DIST_Test').result('test')

    def test_spawn_prefers_ground_over_roofs_and_floating_panels(self):
        def tri(a,b,c):return SimpleNamespace(a=a,b=b,c=c)
        def mesh(*triangles):return SimpleNamespace(bounds_min=(-1e3,-1e3,-1e3),bounds_max=(1e3,1e3,1e3),triangles=list(triangles))
        # Retail park: floor wound downward, roof above it wound upward.
        floor=mesh(tri((-40,0,-40),(40,0,-40),(-40,0,40)),tri((40,0,-40),(40,0,40),(-40,0,40)))
        roof=mesh(tri((-40,30,-40),(-40,30,40),(40,30,-40)),tri((40,30,-40),(-40,30,40),(40,30,40)))
        selector=SpawnSelector('DIST_Park');selector.consider([roof,floor])
        self.assertEqual(selector.result('Park')[1],1.)
        # A small panel over the void in the middle of two floor slabs.
        slabs=mesh(tri((-60,0,-20),(-60,0,20),(-10,0,-20)),tri((10,0,-20),(60,0,20),(60,0,-20)))
        panel=mesh(tri((-2,20,-2),(-2,20,2),(2,20,-2)))
        selector=SpawnSelector('DIST_Park');selector.consider([panel,slabs])
        x,y,z=selector.result('Park')
        self.assertEqual(y,1.)
        self.assertGreater(abs(x),10)
        # City-sized districts keep the original rule: the same roof/floor
        # layout scaled past GROUND_MAX_EXTENT spawns on the upward roof, as
        # before, instead of the "lowest surface" (underground in real cities).
        big_floor=mesh(tri((-400,0,-400),(400,0,-400),(-400,0,400)),tri((400,0,-400),(400,0,400),(-400,0,400)))
        big_roof=mesh(tri((-400,30,-400),(-400,30,400),(400,30,-400)),tri((400,30,-400),(-400,30,400),(400,30,400)))
        selector=SpawnSelector('DIST_City');selector.consider([big_roof,big_floor])
        self.assertEqual(selector.result('City')[1],31.)


if __name__=='__main__':unittest.main()
