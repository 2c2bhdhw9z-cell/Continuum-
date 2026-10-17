import numpy as np
from scipy import ndimage
from common import *
a=load("Cyan")
lab=np.load("cyan_lab.npy")
cx,cy,ri,ro=515.0,499.05,328.7,525.8
end_top,end_bot=-40.31,41.6
axis=743.5
col=fit_colour(a, ndimage.binary_erosion(lab==1,iterations=4))
x0,x1=cx-ro, 2*axis-442
y0,y1=cy-ro, cy+ro
w,h=x1-x0,y1-y0
scale=0.80*1024/max(w,h)
ox=512-scale*(x0+x1)/2; oy=512-scale*(y0+y1)/2
ss=3
X,Y=grid(ss,scale,ox,oy)
inside=(X>=0)&(X<=1023)&(Y>=0)&(Y<=1023)
out=np.zeros(X.shape+(3,))
src=sample(a,np.clip(X,0,1023),np.clip(Y,0,1023))
out[inside]=src[inside]
R=np.hypot(X-cx,Y-cy); A=np.degrees(np.arctan2(Y-cy,X-cx))
inC=(R>=ri)&(R<=ro)&((A<=end_top)|(A>=end_bot))
fill=inC&~inside
out[fill]=col(X[fill],Y[fill])
# The controller's right grip past the old edge, mirrored from its left grip. Only the
# controller's own pixels (component 2, grown by 2 px for its soft edge) are mirrored.
ctrl=ndimage.binary_dilation(lab==2,iterations=2).astype(float)
mir=(~inside)&(X>1023)&(X<=x1+3)&(Y>=0)&(Y<=1023)
mx=np.clip(2*axis-X[mir],0,1023)
k=ndimage.map_coordinates(ctrl,[Y[mir],mx],order=1)
m=sample(a,mx,Y[mir])*k[:,None]
out[mir]=np.maximum(out[mir],m)
save(down(out,ss),"Cyan-new.png")
print("scale",round(scale,4))
