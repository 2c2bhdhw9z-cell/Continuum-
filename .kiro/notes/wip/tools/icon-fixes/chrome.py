import numpy as np
from scipy import ndimage, optimize
from common import *
a=load("Chrome")
tops=np.load("chrome_tops.npy"); lefts=np.load("chrome_lefts.npy")
# The C's outer edge, from where it is in the picture (away from the cut top and left).
px=[];py=[]
for x in list(range(110,270,4))+list(range(800,930,4)): px.append(x); py.append(tops[x]-3)
for y in list(range(110,360,4))+list(range(620,900,4)): px.append(lefts[y]-3); py.append(y)
px=np.array(px,float); py=np.array(py,float)
c=optimize.least_squares(lambda p: np.hypot(px-p[0],py-p[1])-p[2],[500,520,500]).x
cxo,cyo,ro=c
print("outer circle",c.round(1),"err",np.abs(np.hypot(px-cxo,py-cyo)-ro).mean().round(2),"top at",round(cyo-ro,1),"left at",round(cxo-ro,1))
# background: the picture's own gentle top-to-bottom shading, from points well off the art
pts=[(x,y) for x in (8,1015) for y in range(8,1016,40)]+[(512,260),(512,700),(1000,500)]
ys=np.array([p[1] for p in pts]); cols=np.array([a[p[1]-4:p[1]+4,p[0]-4:p[0]+4].reshape(-1,3).mean(0) for p in pts])
k=np.polyfit(ys,cols,1)
from scipy import ndimage as _nd
_art=_nd.binary_dilation(a.mean(2)>34,iterations=22)
_edge=edge_background(a,_art)
def bg_xy(x,y):
    # inside the old picture: its own colour; off it: its own edge background carried out
    return _edge(x,y)
# the art's whole box
x0,x1=cxo-ro,975.0
y0,y1=cyo-ro,1010.0
scale=0.82*1024/max(x1-x0,y1-y0)
ox=512-scale*(x0+x1)/2; oy=512-scale*(y0+y1)/2
print("box",round(x0),round(x1),round(y0),round(y1),"scale",round(scale,4))
ss=2
X,Y=grid(ss,scale,ox,oy)
inside=(X>=0)&(X<=1023)&(Y>=0)&(Y<=1023)
pic=sample(a,np.clip(X,0,1023),np.clip(Y,0,1023))
out=pic.copy()
# Past the old top and left edges: the chrome carried on as its own mirror image, inside the
# C's outer edge, with the same dark rim the picture has where the chrome meets the black.
miss=~inside
D=ro-np.hypot(X-cxo,Y-cyo)          # how far inside the outer edge
mx=np.where(X<0,-X,X); my=np.where(Y<0,-Y,Y)
mirrored=sample(a,np.clip(mx,0,1023),np.clip(my,0,1023))
rim=np.clip(D/14,0,1)[...,None]     # the rim: dark at the very edge, full chrome 14 px in
cov=np.clip(D+0.5,0,1)[...,None]    # anti-aliased edge
b=bg_xy(X,Y)
chrome=b+(mirrored-b)*rim
out[miss]=(b*(1-cov)+chrome*cov)[miss]
# far outside the C (the new margin): the background only
far=miss&(D<-1)
out[far]=b[far]
save(down(out,ss),"Chrome-new.png")
