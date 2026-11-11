import os,tempfile,unittest
from pathlib import Path
from unittest.mock import patch

from tools.asset_pipeline import audio_export


class DecodeCacheTests(unittest.TestCase):
    def setUp(self):
        self.root=Path(tempfile.mkdtemp())
        self.tool=self.root/'vgmstream-cli.exe';self.tool.write_bytes(b'tool v1')
        self.cache=audio_export.DecodeCache(self.root/'installations'/'decode-cache',self.tool)
        self.work=self.root/'work';self.work.mkdir()

    def tearDown(self):
        audio_export._CACHE=None
        import shutil;shutil.rmtree(self.root,ignore_errors=True)

    def fake_vgmstream(self,calls):
        def spawn(args,cwd,**_):
            names=[str(a) for a in args[4:]]
            calls.append(names)
            for name in names:(Path(cwd)/(name+'.wav')).write_bytes(b'RIFF'+(Path(cwd)/name).read_bytes())
            class Process:
                stdout=type('S',(),{'read':lambda self:''})()
                def __enter__(self):return self
                def __exit__(self,*_):return False
                def wait(self):return 0
            return Process()
        return spawn

    def test_key_covers_header_body_and_tool(self):
        (self.work/'a.snr').write_bytes(b'head');(self.work/'b.snr').write_bytes(b'head')
        self.assertEqual(self.cache.key(self.work/'a.snr'),self.cache.key(self.work/'b.snr'))
        (self.work/'b.sns').write_bytes(b'body')
        self.assertNotEqual(self.cache.key(self.work/'a.snr'),self.cache.key(self.work/'b.snr'))
        self.tool.write_bytes(b'tool v2')
        other=audio_export.DecodeCache(self.root/'other',self.tool)
        self.assertNotEqual(other.key(self.work/'a.snr'),self.cache.key(self.work/'a.snr'))

    def test_second_decode_starts_no_process_and_gives_the_same_bytes(self):
        from tools.asset_pipeline import install
        for name in ('0000.snr','0001.snr'):(self.work/name).write_bytes(name.encode())
        audio_export._CACHE=self.cache
        calls=[]
        with patch.object(install,'spawn',self.fake_vgmstream(calls)):
            audio_export._decode(self.tool,self.work,['0000.snr','0001.snr'],None)
            first={n:(self.work/(n+'.wav')).read_bytes() for n in ('0000.snr','0001.snr')}
            again=self.root/'again';again.mkdir()
            for name in ('0000.snr','0001.snr','0002.snr'):(again/name).write_bytes(name.encode())
            audio_export._decode(self.tool,again,['0000.snr','0001.snr','0002.snr'],None)
        self.assertEqual(calls,[['0000.snr','0001.snr'],['0002.snr']])
        for name,data in first.items():self.assertEqual((again/(name+'.wav')).read_bytes(),data)

    def test_prune_keeps_only_entries_used_this_run(self):
        (self.work/'x.snr').write_bytes(b'x');(self.work/'x.snr.wav').write_bytes(b'wav')
        key=self.cache.key(self.work/'x.snr');self.cache.store(key,self.work/'x.snr.wav')
        (self.cache.folder/'stale.wav').write_bytes(b'old')
        self.assertEqual(self.cache.prune(),1)
        self.assertEqual(sorted(p.name for p in self.cache.folder.iterdir()),[key+'.wav'])

    def test_cache_only_inside_an_installations_folder(self):
        private=self.root/'installations'/('0'*32)/'assets'/'private'
        self.assertEqual(audio_export.decode_cache_folder(private),(self.root/'installations'/'decode-cache').resolve())
        self.assertIsNone(audio_export.decode_cache_folder(self.root/'scratch'/'assets'/'private'))
        with patch.dict(os.environ,{'SKATE_SETUP_DECODE_CACHE':'0'}):
            self.assertIsNone(audio_export.decode_cache_folder(private))


if __name__=='__main__':unittest.main()
