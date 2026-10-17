import numpy as np
from scipy import ndimage
from common import *
a=load("Violet")
space=np.array([11.8,5.5,27.3])     # the picture's own darkest space, its top-left corner
L=a.mean(2)
# the controller: its bright outline, closed and filled, grown a little for its own glow
o=(L>110)
o[:, :420]=False; o[:330]=False; o[770:]=False
o=ndimage.binary_closing(o,iterations=6)
lab,n=ndimage.label(o); s=ndimage.sum(o,lab,range(1,n+1)); o=lab==(np.argmax(s)+1)
ctl=ndimage.binary_fill_holes(o)
ys,xs=np.nonzero(ctl); print("controller box",xs.min(),xs.max(),ys.min(),ys.max(),"area",ctl.sum())
keep=ndimage.gaussian_filter(ndimage.binary_dilation(ctl,iterations=14).astype(float),7)
scale=0.84
ox=oy=512-scale*511.5
ss=2
X,Y=grid(ss,scale,ox,oy)
mx=np.where(X<0,-X,np.where(X>1023,1023.0,X))
my=np.where(Y<0,-Y,np.where(Y>1023,2046-Y,Y))
src=sample(a,np.clip(mx,0,1023),np.clip(my,0,1023))
# The nebula fades into space in a soft circle, so there is no square edge anywhere. The
# controller and its glow are kept whole.
U=(np.arange(1024*ss)+0.5)/ss; UX,UY=np.meshgrid(U,U)
r=np.hypot(UX-512,UY-512)
t=np.clip((r-330)/(470-330),0,1); f=1-t*t*(3-2*t)
k=ndimage.map_coordinates(keep,[np.clip(Y,0,1023),np.clip(X,0,1023)],order=1)
k*=(X<=1023+6)   # nothing past the old right edge counts as controller
w=np.maximum(f,k)
out=space+(src-space)*w[...,None]
save(down(out,ss),"Violet-new.png")
