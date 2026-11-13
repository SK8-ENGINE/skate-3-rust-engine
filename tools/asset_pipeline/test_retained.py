import json,tempfile,unittest
from pathlib import Path

from tools.asset_pipeline.retained import carry,kept_dlc_maps


def write(path,data):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(data) if not isinstance(data,str) else data,encoding='utf-8')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


class RetainedTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        root=Path(self.tmp.name)
        self.old,self.new=root/'old',root/'new'
        old,new=self.old,self.new
        # Previous installation: the disc map DownTown and the DLC map SanitariumDLC with all their side data.
        write(old/'teleporters.json',{'format':'skate3rust.teleporters','version':1,'teleporters':[
            {'id':'tele_dt','map':'DownTown'},{'id':'tele_san','map':'SanitariumDLC'}]})
        write(old/'teleports.json',{'version':1,'destinations':[
            {'id':'dist_downtown','map':'DownTown'},{'id':'dist_sanitarium','map':'SanitariumDLC'}]})
        write(old/'park-layouts/worlds.json',{'version':1,'worlds':{
            'dist_downtownpark_full':{'layout':'Downtown_Fullpark','stream':'DIST_DownTown'},
            'dist_san_full':{'layout':'San_Full','stream':'Dist_SanitariumDLC'}}})
        write(old/'park-layouts/San_Full.skate','pieces')
        write(old/'park-layouts/San_Full.props.json','{}')
        write(old/'park-layouts/San_Full2.skate','another layout')
        write(old/'park-layouts/teleporters.json',{'version':1,'teleporters':[{'id':'skpk:San_Full:0','map':'SanitariumDLC'}]})
        write(old/'stock/dlc/dlc_sanitarium-collections.json',{'rows':1})
        write(old/'dlc-unlocks.json',{'version':1,'unlocks':[{'source':'Sanitarium','name':'SAN'}]})
        write(old/'audio/audio_manifest.json',{'version':3,'ambience':{
            '01_dt':{'file':'ambience/01_dt.wav'},'24_dlc_ambience':{'file':'ambience/24_dlc_ambience.wav'}},
            'regions':{'DownTown':{},'SanitariumDLC':{'beds':[24]}},'crossfades':[],
            'banks':{'dlc_bank':[{'file':'banks/dlc_bank/0000.wav','pack':[32,3]}]},'bank_packs':{'dlc_bank':'banks/dlc_bank.pack'}})
        write(old/'audio/ambience/24_dlc_ambience.wav','pcm')
        write(old/'audio/banks/dlc_bank/0000.wav','wav')
        write(old/'audio/banks/dlc_bank.pack','pack')
        # The new run without the DLC folder rebuilt every aggregate from the disc only.
        write(new/'teleporters.json',{'format':'skate3rust.teleporters','version':1,'teleporters':[{'id':'tele_dt','map':'DownTown','new':True}]})
        write(new/'teleports.json',{'version':1,'destinations':[{'id':'dist_downtown','map':'DownTown'}]})
        write(new/'park-layouts/worlds.json',{'version':1,'worlds':{
            'dist_downtownpark_full':{'layout':'Downtown_Fullpark','stream':'DIST_DownTown'}}})
        write(new/'dlc-unlocks.json',{'version':1,'unlocks':[]})
        write(new/'audio/audio_manifest.json',{'version':3,'ambience':{'01_dt':{'file':'ambience/01_dt.wav','new':True}},
            'regions':{'DownTown':{}},'crossfades':[],'banks':{},'bank_packs':{}})

    def tearDown(self):
        self.tmp.cleanup()

    def test_kept_maps_are_the_installed_non_disc_ones(self):
        maps=Path(self.tmp.name)/'maps'
        write(maps/'SanitariumDLC.skate','map')
        write(maps/'DownTown.skate','map')
        catalog=[{'name':'DownTown','path':'maps/DownTown.skate'},{'name':'SanitariumDLC','path':'maps/SanitariumDLC.skate'},
                 {'name':'ZenDLC','path':'maps/ZenDLC.skate'}]
        self.assertEqual(kept_dlc_maps(catalog,['DownTown'],maps),{'SanitariumDLC'})

    def test_a_kept_map_keeps_its_side_data_and_the_new_run_wins(self):
        counts=carry(self.old,self.new,{'SanitariumDLC'})
        self.assertEqual(counts,{'teleporters':1,'teleports':1,'park layouts':2,'collection rows':1,'unlocks':1,'audio entries':4})
        new=self.new
        self.assertEqual([t['id'] for t in read(new/'teleporters.json')['teleporters']],['tele_dt','tele_san'])
        self.assertTrue(read(new/'teleporters.json')['teleporters'][0]['new'],'the new run wrote it: kept as is')
        self.assertEqual([d['id'] for d in read(new/'teleports.json')['destinations']],['dist_downtown','dist_sanitarium'])
        self.assertIn('dist_san_full',read(new/'park-layouts/worlds.json')['worlds'])
        self.assertTrue((new/'park-layouts/San_Full.skate').is_file())
        self.assertTrue((new/'park-layouts/San_Full.props.json').is_file())
        self.assertFalse((new/'park-layouts/San_Full2.skate').exists(),'only the kept layout')
        self.assertEqual(read(new/'park-layouts/teleporters.json')['teleporters'][0]['id'],'skpk:San_Full:0')
        self.assertTrue((new/'stock/dlc/dlc_sanitarium-collections.json').is_file())
        self.assertEqual(read(new/'dlc-unlocks.json')['unlocks'],[{'source':'Sanitarium','name':'SAN'}])
        manifest=read(new/'audio/audio_manifest.json')
        self.assertTrue(manifest['ambience']['01_dt']['new'])
        self.assertIn('24_dlc_ambience',manifest['ambience'])
        self.assertIn('SanitariumDLC',manifest['regions'])
        self.assertTrue((new/'audio/ambience/24_dlc_ambience.wav').is_file())
        self.assertTrue((new/'audio/banks/dlc_bank/0000.wav').is_file())
        self.assertTrue((new/'audio/banks/dlc_bank.pack').is_file(),'a kept bank keeps its pack')

    def test_carrying_twice_adds_nothing_and_no_kept_map_does_nothing(self):
        carry(self.old,self.new,{'SanitariumDLC'})
        self.assertEqual(sum(carry(self.old,self.new,{'SanitariumDLC'}).values()),0)
        self.assertEqual(carry(self.old,self.new,set()),{})


if __name__=='__main__':
    unittest.main()
