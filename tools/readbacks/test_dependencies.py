import copy
from pathlib import Path
import unittest

from ci import test_dependency_graph


class TestDependencyCompatibility(unittest.TestCase):
    def graph(self):
        return {"resolve": {"nodes": [{
            "id": "path+file:///parent/fast3d#1.0.0", "features": ["profiling", "capture"],
            "deps": [{"name": "xxh3", "pkg": "registry+crates.io#twox-hash@2.1.2",
                      "dep_kinds": [{"kind": "dev", "target": None}]}],
        }]}}

    def normalized(self, value):
        return test_dependency_graph(value, Path('/parent'))

    def test_promoting_existing_test_dependency_preserves_test_build_graph(self):
        before = self.graph()
        after = copy.deepcopy(before)
        after['resolve']['nodes'][0]['deps'][0]['dep_kinds'].append({'kind': None, 'target': None})
        self.assertEqual(self.normalized(before), self.normalized(after))

    def test_changed_package_feature_target_or_build_edge_is_incompatible(self):
        before = self.graph()
        for field, value in [('package', 'registry+crates.io#twox-hash@2.2.0'),
                             ('feature', 'new-feature'), ('target', 'cfg(windows)'), ('build', 'build')]:
            with self.subTest(field=field):
                after = copy.deepcopy(before)
                node = after['resolve']['nodes'][0]
                dep = node['deps'][0]
                if field == 'package':
                    dep['pkg'] = value
                elif field == 'feature':
                    node['features'].append(value)
                elif field == 'target':
                    dep['dep_kinds'][0]['target'] = value
                else:
                    dep['dep_kinds'][0]['kind'] = value
                self.assertNotEqual(self.normalized(before), self.normalized(after))

    def test_graph_order_and_checkout_location_are_irrelevant(self):
        before = self.graph()
        after = copy.deepcopy(before)
        node = after['resolve']['nodes'][0]
        node['features'].reverse()
        node['id'] = node['id'].replace('/parent/', '/candidate/')
        self.assertEqual(self.normalized(before), test_dependency_graph(after, Path('/candidate')))

    def test_missing_dependency_is_incompatible(self):
        before = self.graph()
        after = copy.deepcopy(before)
        after['resolve']['nodes'][0]['deps'].clear()
        self.assertNotEqual(self.normalized(before), self.normalized(after))
