"""Full-screen screenshot through the desktop portal, for checking UI changes on GNOME/Wayland,
where gnome-screenshot and friends are refused. Usage: python3 scripts/screenshot-linux.py OUT.png"""
import os
import shutil
import sys
from urllib.parse import unquote, urlparse

import gi

gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib

bus = Gio.bus_get_sync(Gio.BusType.SESSION)
loop = GLib.MainLoop()
result = {}


def on_response(_conn, _sender, _path, _iface, _signal, params):
    code, res = params.unpack()
    result['code'], result['uri'] = code, res.get('uri')
    loop.quit()


token = 'rechimped%d' % GLib.random_int()
sender = bus.get_unique_name()[1:].replace('.', '_')
handle = '/org/freedesktop/portal/desktop/request/%s/%s' % (sender, token)
bus.signal_subscribe(None, 'org.freedesktop.portal.Request', 'Response', handle, None, 0, on_response)
options = {'handle_token': GLib.Variant('s', token), 'interactive': GLib.Variant('b', False)}
bus.call_sync('org.freedesktop.portal.Desktop', '/org/freedesktop/portal/desktop', 'org.freedesktop.portal.Screenshot',
              'Screenshot', GLib.Variant('(sa{sv})', ('', options)), None, 0, -1, None)
GLib.timeout_add_seconds(30, loop.quit)
loop.run()
if result.get('code') != 0 or not result.get('uri'):
    sys.exit('screenshot failed: %r' % result)
# The portal saves into ~/Pictures; move it out of there.
shutil.move(unquote(urlparse(result['uri']).path), os.path.abspath(sys.argv[1]))
