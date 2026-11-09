# Native GTK/WebKit pointer regression probe.
# Run through: node scripts/check-board-drag.mjs --webkit
# Requires GTK 3 and WebKit2 4.1 introspection; briefly opens a fixture-only window.
import gi,json,sys,cairo
gi.require_version('Gtk','3.0');gi.require_version('WebKit2','4.1')
from gi.repository import Gtk,Gdk,GLib,WebKit2
window=Gtk.Window(title='OUTPOST drag regression check');window.set_default_size(1094,688);window.set_decorated(False);window.set_accept_focus(False)
view=WebKit2.WebView();window.add(view)
failed=True

def js(code,done):
 def callback(v,res):
  try: done(json.loads(v.run_javascript_finish(res).get_js_value().to_string()))
  except Exception as e: print('FAIL',repr(e),flush=True);Gtk.main_quit()
 view.run_javascript('JSON.stringify('+code+')',None,callback)

def event(kind,x,y):
 e=Gdk.Event.new(kind);e.window=view.get_window();e.send_event=True;e.time=Gdk.CURRENT_TIME
 e.x=x;e.y=y;e.x_root=x;e.y_root=y
 e.set_device(view.get_display().get_default_seat().get_pointer())
 if kind in [Gdk.EventType.BUTTON_PRESS,Gdk.EventType.BUTTON_RELEASE]:e.button=1
 e.state=Gdk.ModifierType.BUTTON1_MASK if kind!=Gdk.EventType.BUTTON_PRESS else 0
 view.event(e)

def begin(data):
 if not data: print('FAIL no board',flush=True);Gtk.main_quit();return
 print('Native geometry:',data,flush=True)
 r=data;start=(r['x']+r['width']*4.5/8,r['y']+r['height']*6.5/8);end=(start[0],r['y']+r['height']*4.5/8)
 event(Gdk.EventType.BUTTON_PRESS,*start)
 def move():
  event(Gdk.EventType.MOTION_NOTIFY,*end)
  GLib.timeout_add(250,measure);return False
 def measure():
  js("(()=>{const g=document.querySelector('.drag-ghost');if(!g)return null;const r=g.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}})()",measured);return False
 def measured(g):
  if not g:print('FAIL native input did not start dragging',flush=True);Gtk.main_quit();return
  error=max(abs(g['x']+g['width']/2-end[0]),abs(g['y']+g['height']/2-end[1]),abs(g['width']-r['width']/8))
  print(json.dumps({'renderer':'WebKitGTK','zoom':.75,'ghost_error_px':error,'ghost':g,'pointer':end}),flush=True)
  if error>.5:Gtk.main_quit();return
  event(Gdk.EventType.BUTTON_RELEASE,*end)
  GLib.timeout_add(200,verify)
 def verify():
  js("(()=>{const s=window.__outpost.getState();return {uci:s.tree.nodes[s.current].uci,ghost:!!document.querySelector('.drag-ghost')}})()",finished);return False
 def finished(result):
  global failed
  failed=result.get('uci')!='e2e4' or result['ghost']
  print(('FAIL' if failed else 'PASS')+' native drag/drop '+json.dumps(result),flush=True)
  if failed: Gtk.main_quit()
  else: js("(()=>{window.__outpost.getState().setView('tactics');return true})()",lambda _:GLib.timeout_add(700,puzzle_ready))
 GLib.timeout_add(150,move)

def puzzle_ready():
 js("(()=>{const r=document.querySelector('.board').getBoundingClientRect(),a=document.querySelector('.puzzle-layout > .center-col').getBoundingClientRect(),c=document.querySelector('.batch-puzzles').getBoundingClientRect();return {x:r.x,y:r.y,w:r.width,h:r.height,aw:a.width,ah:a.height,footer:c.top,bottom:c.bottom,viewport:innerHeight,stacked:getComputedStyle(document.querySelector('.puzzle-layout')).display==='flex'}})()",puzzle_measured);return False

def puzzle_measured(r):
 global failed
 print('Native puzzle geometry:',json.dumps(r),flush=True)
 failed=r['w']<min(r['aw'],r['ah'])-35 or abs(r['w']-r['h'])>1 or r['y']+r['h']>r['footer'] or (not r['stacked'] and r['bottom']>r['viewport']+1)
 if failed:print('FAIL puzzle sizing',flush=True);Gtk.main_quit();return
 start=(r['x']+r['w']*4.5/8,r['y']+r['h']*7.5/8)
 end=(start[0],r['y']+r['h']*.5/8)
 event(Gdk.EventType.BUTTON_PRESS,*start)
 def move():
  event(Gdk.EventType.MOTION_NOTIFY,*end)
  GLib.timeout_add(150,release);return False
 def release():
  event(Gdk.EventType.BUTTON_RELEASE,*end)
  GLib.timeout_add(300,verify);return False
 def verify():
  js("document.querySelector('.puzzle-status').textContent",finished);return False
 def finished(status):
  global failed
  failed=status!='Solved!'
  print(('FAIL' if failed else 'PASS')+' native puzzle drag/solve: '+status,flush=True)
  if failed: Gtk.main_quit()
  else: js("(()=>{document.querySelector('#puzzle-agent-tab').click();return true})()",lambda _:GLib.timeout_add(200,chat_ready))
 GLib.timeout_add(150,move)

def chat_ready():
 js("(()=>{const c=document.querySelector('#puzzle-agent-panel').getBoundingClientRect(),b=document.querySelector('.board').getBoundingClientRect(),f=document.querySelector('.batch-puzzles').getBoundingClientRect(),i=document.querySelector('#puzzle-agent-panel textarea').getBoundingClientRect();return {viewport:innerWidth,layout:getComputedStyle(document.querySelector('.puzzle-layout')).display,side:document.querySelector('.puzzle-side').getBoundingClientRect().toJSON(),right:c.x>=b.right,bottom:c.bottom,footer:f.top,input:i.bottom,height:i.height,chatHeight:c.height}})()",chat_measured);return False

def chat_measured(r):
 global failed
 failed=(r['layout']!='flex' and not r['right']) or r['bottom']>r['footer']+1 or r['input']>r['bottom']+1 or r['height']<40 or r['chatHeight']<250
 print(('FAIL' if failed else 'PASS')+' native puzzle chat layout: '+json.dumps(r),flush=True)
 Gtk.main_quit()

def ready():
 js("(()=>{if(!window.__outpost)return null;window.__outpost.getState().setEngineOn(false);const r=document.querySelector('.board').getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}})()",begin);return False

def loaded(v,event):
 if event==WebKit2.LoadEvent.FINISHED:GLib.timeout_add(1500,ready)
view.connect('load-changed',loaded);window.show_all()
# Ignore the user's physical mouse while dispatching our own native GDK events.
window.get_window().input_shape_combine_region(cairo.Region(),0,0)
view.load_uri('http://127.0.0.1:1431/preview.html')
GLib.timeout_add_seconds(20,lambda:(print('FAIL native probe timed out',flush=True),Gtk.main_quit(),False)[-1])
Gtk.main();window.destroy();sys.exit(1 if failed else 0)
