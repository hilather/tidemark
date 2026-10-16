#!/usr/bin/env python3
"""Owned by: W7-A. Rasterize actual ANSI captures on a fixed terminal cell grid.
Pango/Cairo uses the mock's 16 px monospace font and 19.5 px line height;
ImageMagick crops and places the resulting images beside the unchanged mock.
No terminal contents are invented or replaced.
"""
import ctypes as C
import ctypes.util
import html
from pathlib import Path
import re
import subprocess
import unicodedata

OUT = Path(__file__).resolve().parent
MOCK = Path('/home/brewerm/git/muse-worktrees/plans/tidemark-tide-chart/mocks/concept-3-tide.png')
PALETTE = ['#000000','#800000','#008000','#808000','#000080','#800080','#008080','#c0c0c0',
           '#808080','#ff0000','#00ff00','#ffff00','#0000ff','#ff00ff','#00ffff','#ffffff']

def indexed(n):
    if n < 16: return PALETTE[n]
    if n >= 232: return '#%02x%02x%02x' % ((8 + 10 * (n - 232),) * 3)
    n -= 16
    values = [0,95,135,175,215,255]
    return '#%02x%02x%02x' % (values[n//36], values[n//6%6], values[n%6])

def runs(raw, mono):
    default_fg, default_bg = ('#dddddd', '#101010') if mono else ('#cfe8ef', '#071526')
    fg, bg, bold, reverse = default_fg, default_bg, False, False
    for part in re.split(r'(\x1b\[[0-9;]*m)', raw):
        if part.startswith('\x1b['):
            codes=[int(v or '0') for v in part[2:-1].split(';')]
            i=0
            while i < len(codes):
                c=codes[i]
                if c == 0: fg,bg,bold,reverse=default_fg,default_bg,False,False
                elif c == 1: bold=True
                elif c == 7: reverse=True
                elif c == 22: bold=False
                elif c == 27: reverse=False
                elif c == 39: fg=default_fg
                elif c == 49: bg=default_bg
                elif 30 <= c <= 37: fg=PALETTE[c-30]
                elif 40 <= c <= 47: bg=PALETTE[c-40]
                elif 90 <= c <= 97: fg=PALETTE[c-90+8]
                elif 100 <= c <= 107: bg=PALETTE[c-100+8]
                elif c in (38,48):
                    if codes[i+1] == 2:
                        color='#%02x%02x%02x' % tuple(codes[i+2:i+5]); i+=4
                    elif codes[i+1] == 5:
                        color=indexed(codes[i+2]); i+=2
                    else: raise ValueError(codes)
                    if c==38: fg=color
                    else: bg=color
                i+=1
        elif part:
            front,back=(bg,fg) if reverse else (fg,bg)
            yield part,front,back,bold

cairo=C.CDLL(ctypes.util.find_library('cairo'))
pango=C.CDLL(ctypes.util.find_library('pango-1.0'))
pc=C.CDLL(ctypes.util.find_library('pangocairo-1.0'))
gobj=C.CDLL(ctypes.util.find_library('gobject-2.0'))
P=C.c_void_p

def bind(lib,name,result,*args):
    fn=getattr(lib,name); fn.restype=result; fn.argtypes=args; return fn

surface_new=bind(cairo,'cairo_image_surface_create',P,C.c_int,C.c_int,C.c_int)
context_new=bind(cairo,'cairo_create',P,P)
antialias=bind(cairo,'cairo_set_antialias',None,P,C.c_int)
source_rgb=bind(cairo,'cairo_set_source_rgb',None,P,C.c_double,C.c_double,C.c_double)
rectangle=bind(cairo,'cairo_rectangle',None,P,C.c_double,C.c_double,C.c_double,C.c_double)
fill=bind(cairo,'cairo_fill',None,P)
move=bind(cairo,'cairo_move_to',None,P,C.c_double,C.c_double)
save=bind(cairo,'cairo_save',None,P)
restore=bind(cairo,'cairo_restore',None,P)
clip=bind(cairo,'cairo_clip',None,P)
write_png=bind(cairo,'cairo_surface_write_to_png',C.c_int,P,C.c_char_p)
destroy=bind(cairo,'cairo_destroy',None,P)
surface_destroy=bind(cairo,'cairo_surface_destroy',None,P)
layout_new=bind(pc,'pango_cairo_create_layout',P,P)
show=bind(pc,'pango_cairo_show_layout',None,P,P)
font_new=bind(pango,'pango_font_description_from_string',P,C.c_char_p)
font_size=bind(pango,'pango_font_description_set_absolute_size',None,P,C.c_double)
font_weight=bind(pango,'pango_font_description_set_weight',None,P,C.c_int)
font_free=bind(pango,'pango_font_description_free',None,P)
set_font=bind(pango,'pango_layout_set_font_description',None,P,P)
set_text=bind(pango,'pango_layout_set_text',None,P,C.c_char_p,C.c_int)
baseline=bind(pango,'pango_layout_get_baseline',C.c_int,P)
unref=bind(gobj,'g_object_unref',None,P)

def color(ctx,value):
    source_rgb(ctx,*[int(value[i:i+2],16)/255 for i in (1,3,5)])

def box(ctx,x,y,w,h,value):
    color(ctx,value); rectangle(ctx,x,y,w,h); fill(ctx)

for source in sorted(OUT.glob('*.ansi')):
    mono=source.stem in ('mono','compact','live-with-stderr')
    raw=source.read_text().rstrip('\n')
    parts=list(runs(raw,mono))
    surface=surface_new(0,1440,900); ctx=context_new(surface); antialias(ctx,1)
    layout=layout_new(ctx); font=font_new(b'JetBrains Mono, DejaVu Sans Mono')
    font_size(font,16*1024)
    box(ctx,0,0,1440,900,'#15161a')
    width=80 if source.stem=='compact' else 140
    lines=raw.count('\n')+1
    left=(1440-width*9.6-20)/2
    bg='#101010' if mono else '#071526'
    box(ctx,left,54,width*9.6+20,lines*19.5+16,bg)
    box(ctx,left,28,width*9.6+20,26,'#2a2b30')
    col=row=0
    for text,fg,bg,bold in parts:
        font_weight(font,700 if bold else 400); set_font(layout,font)
        for char in text:
            if char=='\n': row+=1; col=0; continue
            cells=2 if unicodedata.east_asian_width(char) in ('W','F') else 1
            x=left+10+col*9.6; y=62+row*19.5
            box(ctx,x,y,cells*9.6,19.5,bg)
            if char!=' ':
                save(ctx); rectangle(ctx,x,y,cells*9.6,19.5); clip(ctx)
                encoded=char.encode(); set_text(layout,encoded,len(encoded))
                color(ctx,fg); move(ctx,x,y+15.5-baseline(layout)/1024); show(ctx,layout); restore(ctx)
            col+=cells
    # Caption is outside the terminal, and describes provenance only.
    font_size(font,12*1024); font_weight(font,400); set_font(layout,font)
    for text,x,y in [('Actual tmux capture — '+source.stem,left+12,33),
                     ('Selected-directory history · zero baseline · two label rows · waves inside measured height',180,862)]:
        encoded=text.encode(); set_text(layout,encoded,len(encoded)); color(ctx,'#aaaaaa'); move(ctx,x,y); show(ctx,layout)
    assert write_png(surface,str(source.with_suffix('.png')).encode())==0
    unref(layout); font_free(font); destroy(ctx); surface_destroy(surface)
    # Keep a portable HTML companion using the same captured text and SGR colors.
    content=''.join(f'<span style="color:{fg};background:{bg};font-weight:{700 if bold else 400}">{html.escape(text)}</span>' for text,fg,bg,bold in parts)
    source.with_suffix('.html').write_text(f'<!doctype html><meta charset="utf-8"><style>body{{background:#15161a}}pre{{font:16px/19.5px "JetBrains Mono","DejaVu Sans Mono",monospace;font-variant-ligatures:none}}</style><pre>{content}</pre>')

subprocess.run(['magick',str(MOCK),str(OUT/'projects.png'),'+append',str(OUT/'comparison-full.png')],check=True)
for source,dest in [(MOCK,OUT/'mock-chart.png'),(OUT/'projects.png',OUT/'after-chart.png')]:
    subprocess.run(['magick',str(source),'-crop','1400x240+20+55','+repage',str(dest)],check=True)
subprocess.run(['magick',str(OUT/'mock-chart.png'),str(OUT/'after-chart.png'),'+append',str(OUT/'comparison-chart.png')],check=True)
