"""Offline tests for the workbench's binary-freshness record (#103); they
write files in a temporary directory and never build or launch anything.

Run: python -m unittest discover -s scripts/visual-workbench -p "test_*.py"
"""
import tempfile
import unittest
from pathlib import Path

import freshness


class Digest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        (self.root / "crates" / "pane" / "src").mkdir(parents=True)
        (self.root / "crates" / "pane" / "src" / "lib.rs").write_text("pub fn a() {}\n")
        (self.root / "Cargo.lock").write_text("lock\n")
        self.files = ["crates/pane/src/lib.rs", "Cargo.lock"]

    def test_the_same_tree_gives_the_same_digest_in_any_listing_order(self):
        self.assertEqual(freshness.digest_files(self.root, self.files),
                         freshness.digest_files(self.root, list(reversed(self.files))))

    def test_an_edited_source_changes_the_digest(self):
        before = freshness.digest_files(self.root, self.files)
        (self.root / "crates" / "pane" / "src" / "lib.rs").write_text("pub fn b() {}\n")
        self.assertNotEqual(freshness.digest_files(self.root, self.files), before)

    def test_a_deleted_or_added_source_changes_the_digest(self):
        before = freshness.digest_files(self.root, self.files)
        (self.root / "crates" / "pane" / "src" / "lib.rs").unlink()
        deleted = freshness.digest_files(self.root, self.files)
        self.assertNotEqual(deleted, before)
        (self.root / "crates" / "pane" / "src" / "lib.rs").write_text("pub fn a() {}\n")
        (self.root / "crates" / "pane" / "src" / "new.rs").write_text("\n")
        self.assertNotEqual(freshness.digest_files(self.root, self.files + ["crates/pane/src/new.rs"]), before)

    def test_the_sidecar_sits_beside_the_binary(self):
        self.assertEqual(freshness.sidecar_for(Path("target/debug/pane-visual-fixture.exe")),
                         Path("target/debug/pane-visual-fixture.inputs.json"))


class Verdict(unittest.TestCase):
    RECORD = {"binarySha256": "B" * 64, "inputsSha256": "I" * 64}

    def test_the_binary_the_workbench_built_from_these_sources_is_fresh(self):
        self.assertIsNone(freshness.verdict(self.RECORD, "B" * 64, "I" * 64))

    def test_a_binary_without_a_build_record_is_refused(self):
        self.assertIn("not built by the workbench", freshness.verdict(None, "B" * 64, "I" * 64))

    def test_a_binary_rebuilt_or_replaced_outside_the_workbench_is_refused(self):
        self.assertIn("outside the workbench", freshness.verdict(self.RECORD, "C" * 64, "I" * 64))

    def test_a_binary_older_than_the_sources_is_refused(self):
        self.assertIn("older than the tree", freshness.verdict(self.RECORD, "B" * 64, "J" * 64))


if __name__ == "__main__":
    unittest.main()
