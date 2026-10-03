import struct
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
from skate3_streams import AssetRecord, StreamFormatError, read_sfil


class StreamTests(unittest.TestCase):
    def fixture(self, tail=b'\0'*128):
        raw=bytearray(384)
        raw[:4]=b'SFIL';struct.pack_into('>I',raw,16,128)
        struct.pack_into('>Q3I',raw,128,123,32,128,256)
        raw[256:288]=bytes(range(32))
        record=AssetRecord(123,0,32,128,128,0,4,b'')
        return bytes(raw)+tail,record

    def test_raw_assets_padding_and_shared_index(self):
        class NoSuffixCopy(bytes):
            def __getitem__(self,key):
                if isinstance(key,slice) and key.start and key.stop is None:
                    raise AssertionError('Copied the entire remaining stream')
                return super().__getitem__(key)
        raw,record=self.fixture()
        with patch.object(Path,'read_bytes',return_value=NoSuffixCopy(raw)):
            normal=read_sfil('fixture.xsf',[record])
            shared=read_sfil('fixture.xsf',[record],record_index={123:record})
        self.assertEqual(normal,shared)
        self.assertEqual(normal[0].data,bytes(range(32)))
        self.assertEqual(normal[0].source_offset,128)

    def test_truncated_nonzero_tail_is_still_rejected(self):
        raw,record=self.fixture(b'\0\1')
        with patch.object(Path,'read_bytes',return_value=raw):
            with self.assertRaisesRegex(StreamFormatError,'truncated'):
                read_sfil('fixture.xsf',[record])

    def test_missing_and_unknown_records_are_still_rejected(self):
        raw,record=self.fixture()
        missing=AssetRecord(456,0,32,128,128,0,4,b'')
        with patch.object(Path,'read_bytes',return_value=raw):
            with self.assertRaisesRegex(StreamFormatError,'missing'):
                read_sfil('fixture.xsf',[record,missing])
            self.assertEqual(len(read_sfil('fixture.xsf',[record,missing],require_all_records=False)),1)
            with self.assertRaisesRegex(StreamFormatError,'absent'):
                read_sfil('fixture.xsf',[missing])


    def test_identical_copies_from_other_cells_are_skipped_not_decoded(self):
        raw,record=self.fixture()
        known={}
        with patch.object(Path,'read_bytes',return_value=raw):
            first=read_sfil('a.xsf',[record],record_index={123:record},known_copies=known)
            self.assertEqual(len(first),1)
            self.assertIn(123,known)
            with patch('skate3_streams._decode_section',side_effect=AssertionError('decoded a known copy')):
                again=read_sfil('b.xsf',[record],record_index={123:record},known_copies=known,
                                require_all_records=False)
        self.assertEqual(again,[])

    def test_differing_copies_are_still_decoded_for_the_conflict_check(self):
        raw,record=self.fixture()
        changed=bytearray(raw);changed[256]^=0xff
        known={}
        with patch.object(Path,'read_bytes',return_value=raw):
            read_sfil('a.xsf',[record],known_copies=known)
        with patch.object(Path,'read_bytes',return_value=bytes(changed)):
            other=read_sfil('b.xsf',[record],known_copies=known)
        self.assertEqual(len(other),1)
        self.assertNotEqual(other[0].data,bytes(range(32)))

    def test_duplicates_within_one_file_are_still_rejected_with_known_copies(self):
        raw,record=self.fixture()
        doubled=bytearray(raw[:384])+raw[128:384]
        with patch.object(Path,'read_bytes',return_value=bytes(doubled)):
            with self.assertRaisesRegex(StreamFormatError,'duplicate'):
                read_sfil('a.xsf',[record],known_copies={})


if __name__=='__main__':unittest.main()
