"""Release guards must fail before either registry receives an upload."""

import json
from pathlib import Path
import tempfile
import unittest

from release_version import ROOT, validate


class ReleaseManifestTests(unittest.TestCase):
    def test_release_please_manifest_matches_component_versions(self):
        config = json.loads((ROOT / "release-please-config.json").read_text())
        manifest = json.loads((ROOT / ".release-please-manifest.json").read_text())
        self.assertTrue(set(manifest).issubset(config["packages"]))
        for path, package in config["packages"].items():
            with self.subTest(path=path):
                component = package["component"]
                self.assertEqual(component, path)
                version = manifest.get(path, package["initial-version"])
                validate(component, f"{component}-v{version}")


class ReleaseVersionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.nodes = self.root / "nodes"
        self.nodes.mkdir(parents=True)
        self.gateway = self.root / "gateway"
        self.gateway.mkdir(parents=True)
        self.write_nodes("0.2.0")
        self.write_gateway("0.3.0")

    def write_nodes(self, version, lock_version=None):
        (self.nodes / "package.json").write_text(json.dumps({"version": version}))
        (self.nodes / "package-lock.json").write_text(json.dumps({
            "version": lock_version or version, "packages": {"": {"version": version}},
        }))

    def write_gateway(self, version, lock_version=None):
        (self.gateway / "Cargo.toml").write_text(f'[package]\nname = "gateway"\nversion = "{version}"\n')
        (self.gateway / "Cargo.lock").write_text(f'[[package]]\nname = "gateway"\nversion = "{lock_version or version}"\n')

    def test_independent_stable_versions(self):
        for component, version in [("nodes", "0.2.0"), ("gateway", "0.3.0")]:
            with self.subTest(component=component):
                self.assertEqual(validate(component, f"{component}-v{version}", self.root),
                                 {"version": version, "stable": "true", "npm_tag": "latest"})

    def test_prereleases_do_not_use_latest(self):
        self.write_nodes("0.2.0-rc.1")
        self.write_gateway("0.3.0-beta.2")
        for component, version in [("nodes", "0.2.0-rc.1"), ("gateway", "0.3.0-beta.2")]:
            with self.subTest(component=component):
                result = validate(component, f"{component}-v{version}", self.root)
                self.assertEqual(result["stable"], "false")
                self.assertEqual(result["npm_tag"], "next")

    def test_wrong_component_and_legacy_tags_rejected(self):
        for component, tag in [("nodes", "gateway-v0.2.0"), ("gateway", "nodes-v0.3.0"),
                               ("nodes", "0.2.0"), ("gateway", "v0.3.0")]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                validate(component, tag, self.root)

    def test_invalid_versions_rejected(self):
        for version in ["01.2.0", "0.2", "0.2.0-rc.01", "0.2.0+build", "0.2.0-", "0.2.0\n"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                validate("nodes", f"nodes-v{version}", self.root)

    def test_tag_manifest_mismatch_rejected(self):
        for component in ["nodes", "gateway"]:
            with self.subTest(component=component), self.assertRaises(ValueError):
                validate(component, f"{component}-v9.9.9", self.root)

    def test_stale_lockfiles_rejected(self):
        self.write_nodes("0.2.0", "0.1.0")
        self.write_gateway("0.3.0", "0.1.0")
        for component, version in [("nodes", "0.2.0"), ("gateway", "0.3.0")]:
            with self.subTest(component=component), self.assertRaises(ValueError):
                validate(component, f"{component}-v{version}", self.root)

    def test_stale_npm_root_entry_rejected(self):
        path = self.nodes / "package-lock.json"
        lock = json.loads(path.read_text())
        lock["packages"][""]["version"] = "0.1.0"
        path.write_text(json.dumps(lock))
        with self.assertRaises(ValueError):
            validate("nodes", "nodes-v0.2.0", self.root)


if __name__ == "__main__":
    unittest.main()
