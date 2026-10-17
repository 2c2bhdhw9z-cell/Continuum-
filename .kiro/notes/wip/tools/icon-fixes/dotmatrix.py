import numpy as np
from common import *
a=load("Dotmatrix")
bg=np.array([13.0,11.5,12.5])         # the picture's own black, measured
x0,x1,y0,y1=168,995,273,785           # the controller's dots
scale=0.93
cx,cy=(x0+x1)/2,(y0+y1)/2
ox=512-scale*cx; oy=512-scale*cy
ss=2
X,Y=grid(ss,scale,ox,oy)
src=sample(a,np.clip(X,0,1023),np.clip(Y,0,1023))
# Keep the controller and its own glow; let everything else (the cut-off dot frame on the
# left and the glow along the right edge) fade into the picture's own black.
def ramp(d,w): 
    t=np.clip(d/w,0,1); return t*t*(3-2*t)
L,R,T,B=x0-55,x1+22,y0-55,y1+55
m=ramp(X-L,40)*ramp(R-X,22)*ramp(Y-T,40)*ramp(B-Y,40)
out=bg+(src-bg)*m[...,None]
save(down(out,ss),"Dotmatrix-new.png")
