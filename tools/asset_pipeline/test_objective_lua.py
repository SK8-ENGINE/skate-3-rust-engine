import unittest

from tools.asset_pipeline.objective_lua import translate


class ObjectiveLua(unittest.TestCase):
    def test_a_pure_expression(self):
        p = translate('do return Lib.InVolume("123") and not Lib.Bailed() end')
        self.assertEqual(p['body'], [{'return': {'and': [
            {'api': 'Lib.InVolume', 'args': [{'lit': '123'}]},
            {'not': {'api': 'Lib.Bailed', 'args': []}}]}}])

    def test_vars_state_and_init(self):
        p = translate('''do
            InitVars(vars, "Count", 0)
            if Lib.Done() then vars.Count = vars.Count + 1 end
            return vars["Count"] >= 2
        end''')
        self.assertEqual(p['body'][0], {'init_var': 'Count', 'value': {'lit': 0}})
        self.assertEqual(p['body'][1]['if'][0][1], [{'set_var': 'Count', 'value': {
            'arith': 'add', 'args': [{'var': 'Count'}, {'lit': 1}]}}])
        self.assertEqual(p['body'][2], {'return': {'cmp': 'ge', 'args': [{'var': 'Count'}, {'lit': 2}]}})

    def test_locals_scopes_and_for(self):
        p = translate('''local s = 0
            for i = 1, 3 do local t = i; s = s + t end
            if s == 6 then return true elseif s > 6 then return false else return nil end''')
        self.assertEqual(p['locals'], 3)
        loop = p['body'][1]
        self.assertEqual((loop['for'], loop['step']), (1, {'lit': 1}))
        self.assertEqual(loop['body'][0], {'set_local': 2, 'value': {'local': 1}})
        self.assertEqual(len(p['body'][2]['if']), 2)

    def test_enums_globals_and_comments(self):
        p = translate('''-- a line comment
            --[[ a block
            comment ]]
            Seen = true
            return Flow.GetStatus(0) == Flow.eObjectiveStatus.Complete and Seen''')
        self.assertEqual(p['body'][0], {'set_global': 'Seen', 'value': {'lit': True}})
        self.assertEqual(p['body'][1]['return']['and'][0]['args'][1], {'lit': 8})
        self.assertEqual(translate('return Lib.EOther.Thing')['body'][0], {'return': {'enum': 'Lib.EOther.Thing'}})

    def test_lua_errors_stay_errors(self):
        broken = translate('do return Lib.Done( end')
        self.assertIn('syntax_error', broken)
        self.assertEqual(broken['body'][0]['call']['missing'][:12], 'syntax error')
        self.assertEqual(translate('InitVars.flag = true')['body'], [{'call': {'missing': 'assignment to a field'}}])
        self.assertEqual(translate('')['body'], [], 'empty text: no result, the default')

    def test_numbers_strings_and_tables(self):
        p = translate('return Lib.Pick({1, 0x10, 2.5, -3}, "a\\"b", ' + "'c')")
        args = p['body'][0]['return']['args']
        self.assertEqual(args[0], {'table': [{'lit': 1}, {'lit': 16}, {'lit': 2.5}, {'neg': {'lit': 3}}]})
        self.assertEqual((args[1], args[2]), ({'lit': 'a"b'}, {'lit': 'c'}))

    def test_fields_of_a_local_table(self):
        p = translate('local t = Lib.Shared(1); return t.count ~= 0')
        self.assertEqual(p['body'][1], {'return': {'cmp': 'ne', 'args': [
            {'index': [{'local': 0}, {'lit': 'count'}]}, {'lit': 0}]}})

    def test_outside_the_subset_is_reported(self):
        for text in ('return a * b', 'local function f() end', 'while true do end', 'return x:y()'):
            self.assertIn('syntax_error', translate(text), text)


if __name__ == '__main__':
    unittest.main()
