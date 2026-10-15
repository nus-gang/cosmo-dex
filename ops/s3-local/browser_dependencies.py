"""Read-only pins for existing Node and the complete installed Chrome bundle.

No download, installation or browser execution. Internal bundle symlink targets
are recorded; links leaving the bundle and other special files reject.
"""
import json
import os
from pathlib import Path
import stat
import sys
from preflight import checked_root,file_digest


def snapshot(root):
    root=checked_root(root);files={};links={}
    for directory,dirs,names in os.walk(root,followlinks=False):
        for name in sorted(dirs+names):
            p=Path(directory)/name;s=p.lstat();rel=p.relative_to(root).as_posix()
            if stat.S_ISLNK(s.st_mode):
                target=os.readlink(p)
                if Path(target).is_absolute() or not p.resolve(strict=True).is_relative_to(root):raise ValueError('BROWSER_LINK')
                links[rel]=target
                if name in dirs:dirs.remove(name)
            elif stat.S_ISREG(s.st_mode):files[rel]=file_digest(root,rel)
            elif not stat.S_ISDIR(s.st_mode):raise ValueError('BROWSER_SPECIAL_FILE')
            if len(files)+len(links)>4096:raise ValueError('BROWSER_BUNDLE_LIMIT')
    if not files:raise ValueError('BROWSER_BUNDLE_EMPTY')
    return dict(files_sha256=files,symlinks=links)


def capture(node,chrome):
    node=Path(node);chrome=Path(chrome)
    if str(node.resolve(strict=True))!=str(node) or str(chrome.resolve(strict=True))!=str(chrome) or chrome.parent.name!='MacOS':
        raise ValueError('BROWSER_EXECUTABLE')
    root=chrome.parent.parent
    value=dict(schema='s3-local-browser-dependencies/1',node=dict(path=str(node),sha256=file_digest(node.parent,node.name)),
        chrome=dict(path=str(chrome),sha256=file_digest(chrome.parent,chrome.name)),
        chrome_bundle=dict(root=str(root),**snapshot(root)))
    return value


def verify(value):
    if set(value)!={'schema','node','chrome','chrome_bundle'} or value['schema']!='s3-local-browser-dependencies/1':raise ValueError('BROWSER_RUNTIME')
    if capture(value['node']['path'],value['chrome']['path'])!=value:raise ValueError('BROWSER_RUNTIME_CHANGED')


if __name__=='__main__':
    if len(sys.argv)!=3:raise SystemExit(2)
    print(json.dumps(capture(*sys.argv[1:]),sort_keys=True,separators=(',',':')))
