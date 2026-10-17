import numpy as np
from scipy import ndimage
from common import *
a=load("Spectrum")
sat=(a.max(2)-a.min(2))/np.maximum(a.max(2),1)
ring=(sat>0.5)&(a.max(2)>120)
lab,n=ndimage.label(ring); sizes=ndimage.sum(ring,lab,range(1,n+1))
ring=lab==(np.argmax(sizes)+1)
ring=ndimage.binary_erosion(ring,iterations=3)   # away from the soft block edges
ctrl=np.load("spec_ctrl.npy")
c0=511.5
ys,xs=np.nonzero(ring)
ang=np.degrees(np.arctan2(ys-c0,xs-c0))%360
rad=np.hypot(xs-c0,ys-c0)
cols=a[ys,xs]
# the ring's own colours by angle, outer and inner layer, 4 degree steps
bins=np.arange(0,360,4)
def table(sel):
    t={}
    for b in bins:
        m=sel&(ang>=b)&(ang<b+4)
        if m.sum()>30: t[b]=np.median(cols[m],0)
    return t
# a ring pixel is "outer" when it is in the outer half of the ring at its angle
med=np.zeros_like(rad)
for b in bins:
    m=(ang>=b)&(ang<b+4)
    if m.any(): med[m]=np.median(rad[m])
outer_t=table(rad>=med); inner_t=table(rad<med)
def colour(t,theta):
    keys=np.array(sorted(t)); d=np.abs(((keys+2)-theta+180)%360-180)
    return t[keys[np.argmin(d)]]
# the new C: square pixel blocks on a grid with a corner at the centre
B=50; RO=432; RI=RO-2.5*B; OPEN=36.0
out=np.zeros((1024,1024,3))
k0=int(np.ceil(RO/B))+1
for j in range(-k0,k0):
    for i in range(-k0,k0):
        bx=512+i*B+B/2; by=512+j*B+B/2
        d=np.hypot(bx-512,by-512); th=np.degrees(np.arctan2(by-512,bx-512))%360
        if not (RI<=d<RO): continue
        if th<OPEN or th>360-OPEN: continue
        t=outer_t if d>=(RI+RO)/2 else inner_t
        x0,y0=int(512+i*B),int(512+j*B)
        out[max(y0,0):y0+B,max(x0,0):x0+B]=colour(t,th)
# the controller, as it is, pixel for pixel, a little right of centre as in the original
ys,xs=np.nonzero(ctrl); cx0=(xs.min()+xs.max())/2; cy0=(ys.min()+ys.max())/2
grow=ndimage.binary_dilation(ctrl,iterations=6)
dx=int(round(512+70-cx0)); dy=int(round(512-cy0))
Y,X=np.nonzero(grow)
out[Y+dy,X+dx]=a[Y,X]
save(out,"Spectrum-new.png")
print("ring box", 512-RO, 512+RO, "controller x",xs.min()+dx,xs.max()+dx)
# A little more room round the C, like the other icons: the whole picture at 92%, on black.
from PIL import Image
im=Image.open("Spectrum-new.png")
s=int(round(1024*0.92))
small=im.resize((s,s),Image.LANCZOS)
canvas=Image.new("RGB",(1024,1024),(0,0,0)); canvas.paste(small,((1024-s)//2,(1024-s)//2))
canvas.save("Spectrum-new.png")
