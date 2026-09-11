import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
from pathlib import Path

from browser_processes import BrowserProcesses


class BrowserProcessTests(unittest.TestCase):
    def test_teardown_waits_for_orphan_and_term_resistant_child_on_delete_error(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            profile = out/'chrome-profile'
            child_file = out/'child.json'
            child = "import json,os,signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); print(json.dumps(dict(pid=os.getpid(),token=os.environ.get('FAST3D_BROWSER_RUN_ID'))),flush=True); time.sleep(60)"
            parent = "import json,subprocess,sys; from pathlib import Path; p=subprocess.Popen([sys.executable,'-c',sys.argv[1]],start_new_session=True,stdout=subprocess.PIPE,text=True); Path(sys.argv[3]).write_text(p.stdout.readline());"
            unrelated = subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)',str(profile)+'-backup'])
            tree = None
            def controlled_inventory():
                processes = {}
                candidates = [(unrelated.pid, os.getpid(), unrelated.pid, 'python --user-data-dir='+str(profile)+'-backup')]
                for child in tree.children if tree else []:
                    if child.poll() is None:
                        candidates.append((child.pid, os.getpid(), child.pid, 'python driver'))
                if child_file.exists():
                    pid = json.loads(child_file.read_text())['pid']
                    candidates.append((pid, 1, pid, 'python detached-helper'))
                for pid, ppid, pgid, command in candidates:
                    try:
                        os.kill(pid, 0)
                    except ProcessLookupError:
                        continue
                    processes[pid] = {'pid':pid,'ppid':ppid,'pgid':pgid,'state':'S',
                                      'started':'Fri Sep 11 17:00:00 2026','command':command}
                return processes
            def controlled_tokens(pids, token):
                if child_file.exists():
                    child_data = json.loads(child_file.read_text())
                    if child_data['pid'] in pids and child_data['token'] == token:
                        return {child_data['pid']}
                return set()
            inventory_patch = patch('browser_processes.snapshot', side_effect=controlled_inventory)
            token_patch = patch('browser_processes.token_processes', side_effect=controlled_tokens, create=True)
            if not os.environ.get('PERF_LIVE_PROCESS_TESTS'):
                inventory_patch.start()
                token_patch.start()
            tree = BrowserProcesses(out, profile, grace_seconds=1)
            try:
                tree.start([sys.executable,'-c',parent,child,str(profile),str(child_file)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                deadline = time.monotonic()+5
                while not child_file.exists() and time.monotonic()<deadline:
                    time.sleep(.02)
                self.assertTrue(child_file.exists())
                child_pid = json.loads(child_file.read_text())['pid']
                def failed_delete():
                    raise RuntimeError('DELETE failed')
                tree.close(failed_delete)
                report = json.loads((out/'browser-teardown.json').read_text())
                self.assertTrue(report['complete'], report)
                self.assertIn(child_pid, [p['pid'] for p in report['observed']])
                self.assertIn('DELETE failed', report['session_delete_error'])
                self.assertEqual(report['survivors'], [])
                self.assertIsNone(unrelated.poll())
                with self.assertRaises(ProcessLookupError):
                    os.kill(child_pid, 0)
            finally:
                tree.close()
                inventory_patch.stop()
                token_patch.stop()
                if child_file.exists():
                    try:
                        os.kill(json.loads(child_file.read_text())['pid'], signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                unrelated.terminate()
                unrelated.wait(timeout=5)


if __name__ == '__main__':
    unittest.main()
