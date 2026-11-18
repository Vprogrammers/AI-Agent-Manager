import pathlib
import sys
import unittest
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'src'))

class PromptKeyTests(unittest.TestCase):
    def test_enter_sends_and_ctrl_n_inserts_newline(self):
        try:
            from aam_textual import PromptEditor
        except ImportError:
            self.skipTest('textual optional dependency not installed')
        bindings = {binding.key: binding.action for binding in PromptEditor.BINDINGS}
        self.assertEqual(bindings['enter'], 'submit_prompt')
        self.assertEqual(bindings['ctrl+n'], 'insert_newline')

if __name__ == '__main__':
    unittest.main()
