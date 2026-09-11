import json
import os
import re
import signal
import subprocess
import threading
import time
import uuid

PROCESS_COMMAND = ['ps','-ww','-axo','pid=,ppid=,pgid=,stat=,lstart=,args=']
TOKEN_COMMAND = ['ps','-Eww','-o','pid=,command=','-p']


def snapshot():
    raw = subprocess.run(PROCESS_COMMAND,
                         check=True, capture_output=True, text=True, timeout=5,
                         env={**os.environ, 'LC_ALL':'C'}).stdout
    processes = {}
    for line in raw.splitlines():
        fields = line.split(maxsplit=9)
        pid, ppid, pgid = map(int, fields[:3])
        processes[pid] = {'pid':pid, 'ppid':ppid, 'pgid':pgid, 'state':fields[3],
                          'started':' '.join(fields[4:9]), 'command':fields[9]}
    return processes


def token_processes(pids, token):
    if not pids:
        return set()
    result = subprocess.run(TOKEN_COMMAND+[','.join(map(str, sorted(pids)))],
                            capture_output=True, text=True, timeout=5)
    if result.returncode not in (0, 1) or result.stderr.strip():
        raise RuntimeError(f'cannot inspect browser ownership: {result.stderr}')
    marker = 'FAST3D_BROWSER_RUN_ID='+token
    return {int(line.split(maxsplit=1)[0]) for line in result.stdout.splitlines()
            if marker in line.split()}


class BrowserProcesses:
    def __init__(self, out, profile, grace_seconds=5):
        self.out = out
        self.profile = str(profile)
        self.token = uuid.uuid4().hex
        self.grace_seconds = grace_seconds
        self.children = []
        self.observed = {}
        self.initial = {(pid, p['started']) for pid, p in snapshot().items()}
        self.checked = set()
        self.unclaimed = []
        self.errors = []
        self.lock = threading.Lock()
        self.stopped = threading.Event()
        self.closed = False
        self.thread = threading.Thread(target=self.watch, daemon=True)
        self.thread.start()

    def start(self, args, **kwargs):
        with self.lock:
            child = subprocess.Popen(args, start_new_session=True,
                                     env={**os.environ, 'FAST3D_BROWSER_RUN_ID':self.token}, **kwargs)
            self.children.append(child)
        return child

    def collect(self):
        with self.lock:
            processes = snapshot()
            roots = {p.pid for p in self.children if p.poll() is None}
            groups = roots | {p['pgid'] for pid, p in processes.items()
                              if (pid, p['started']) in self.observed}
            owned = {pid for pid, p in processes.items()
                     if (pid, p['started']) in self.observed or p['pgid'] in groups}
            candidates = {pid for pid, p in processes.items() if pid not in owned
                          and (pid, p['started']) not in self.initial | self.checked}
            owned |= token_processes(candidates, self.token)
            self.checked |= {(pid, processes[pid]['started']) for pid in candidates}
            owned |= roots
            while True:
                children = {pid for pid, p in processes.items() if p['ppid'] in owned}
                if children <= owned:
                    break
                owned |= children
            self.unclaimed = [p for pid, p in processes.items() if pid not in owned
                              and (pid, p['started']) not in self.initial
                              and not p['state'].startswith('Z')
                              and re.search(r'Google Chrome|Chromium|chrome_crashpad_handler|chromedriver', p['command'], re.I)]
            live = []
            for pid in owned & processes.keys():
                process = processes[pid]
                self.observed[(pid, process['started'])] = process
                if not process['state'].startswith('Z'):
                    live.append(process)
            return live

    def watch(self):
        while not self.stopped.wait(.5):
            try:
                self.collect()
            except Exception as error:
                self.errors.append(str(error))
                return

    def signal_live(self, live, sig):
        current = snapshot()
        for process in live:
            pid = process['pid']
            if pid in current and current[pid]['started'] == process['started']:
                try:
                    os.kill(pid, sig)
                except ProcessLookupError:
                    pass

    def wait_for_exit(self, seconds):
        deadline = time.monotonic()+seconds
        while True:
            live = self.collect()
            if not live or time.monotonic() >= deadline:
                return live
            time.sleep(.1)

    def close(self, delete_session=None):
        if self.closed:
            return
        delete_error = None
        survivors = []
        try:
            try:
                self.collect()
            except Exception as error:
                self.errors.append(str(error))
            if delete_session:
                try:
                    delete_session()
                except Exception as error:
                    delete_error = str(error)
            for child in self.children:
                if child.poll() is None:
                    child.terminate()
            self.signal_live(self.collect(), signal.SIGTERM)
            survivors = self.wait_for_exit(self.grace_seconds)
            if survivors:
                self.signal_live(survivors, signal.SIGKILL)
                survivors = self.wait_for_exit(self.grace_seconds)
        except Exception as error:
            self.errors.append(str(error))
        finally:
            for child in self.children:
                try:
                    child.wait(timeout=self.grace_seconds)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=self.grace_seconds)
            self.stopped.set()
            self.thread.join(timeout=6)
            if self.thread.is_alive():
                self.errors.append('process tracker did not stop')
            try:
                survivors = self.collect()
            except Exception as error:
                self.errors.append(str(error))
            report = {'complete':not survivors and not self.errors and not self.unclaimed, 'profile':self.profile,
                      'session_delete_error':delete_error, 'errors':self.errors,
                      'process_command':PROCESS_COMMAND, 'ownership_command':TOKEN_COMMAND+['<new PIDs>'],
                      'observed':list(self.observed.values()), 'survivors':survivors,
                      'unclaimed_new_browser_processes':self.unclaimed}
            (self.out/'browser-teardown.json').write_text(json.dumps(report, indent=2))
            self.closed = True
        if not report['complete']:
            names = ', '.join(f"{p['command']} (pid {p['pid']})" for p in survivors+self.unclaimed)
            raise RuntimeError(f"browser teardown incomplete: {names}; {self.errors}")
