import struct
import unittest

from .teleporters import label_of, parse_exits, parse_trigger


def strings(*texts):
    """A vault .bin string block: the texts NUL-terminated, returning (binary, offsets)."""
    binary, offsets = b'', []
    for text in texts:
        offsets.append(len(binary))
        binary += text.encode() + b'\0'
    return binary, offsets


class TeleporterTests(unittest.TestCase):
    def test_the_trigger_names_district_volume_and_its_id(self):
        binary, (at,) = strings('DIST_University|tele_world_to_stadium_volume_01|0x2c701706002605aa')
        raw = struct.pack('>IIQ', at, 0, 0x2C701706002605AA)
        self.assertEqual(parse_trigger(raw, binary),
                         ('DIST_University', 'tele_world_to_stadium_volume_01', 0x2C701706002605AA))
        with self.assertRaises(ValueError):
            parse_trigger(struct.pack('>IIQ', at, 0, 1), binary)

    def test_exits_name_the_target_world_and_the_locator(self):
        binary, (_, at) = strings('x', 'tele_world_to_stadium_dest_locator_01')
        item = struct.pack('>QQII', 0xBC69C54E6E102D88, 0xA4F349111C442BC4, 0, 0) + struct.pack('>II', at, 0)
        self.assertEqual(parse_exits([item.hex()], binary), [(0xA4F349111C442BC4, 'tele_world_to_stadium_dest_locator_01')])
        with self.assertRaises(ValueError):
            parse_exits([item[:24].hex()], binary)

    def test_map_labels_drop_the_district_prefix(self):
        self.assertEqual([label_of(d) for d in ('DIST_MegaPark', 'Dist_Zen', 'DLC_DW_MegaCompund')],
                         ['MegaPark', 'Zen', 'DLC_DW_MegaCompund'])


if __name__ == '__main__':
    unittest.main()
