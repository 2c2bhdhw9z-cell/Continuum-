import numpy as np
from scipy import ndimage
from common import *
a=load("Mark")
cx,cy,RI,RO=630.4,505.9,450.0,670.0
RM=(RI+RO)/2; CAP=(RO-RI)/2
# The arms end in round caps just past the old right edge: placed so each cap's inner side
# meets the edge exactly where the picture's own curl does (y 192 and y 823 at x 1023).
TH_TOP,TH_BOT=np.radians(-48.8),np.radians(49.0)
caps=[(cx+RM*np.cos(t),cy+RM*np.sin(t)) for t in (TH_TOP,TH_BOT)]
# ---- the band's colour along each radius, read from the picture where it is visible
radii=np.arange(int(RI),int(RO)+1)
angs=np.radians(np.arange(-180,180,0.25))
RR,AA=np.meshgrid(radii,angs,indexing='ij')
XX=cx+RR*np.cos(AA); YY=cy+RR*np.sin(AA)
vis=(XX>=0)&(XX<=1023)&(YY>=0)&(YY<=1023)
# Only the plain stretch of the band counts as a source: near its ends the band curls into the
# mouth, and its colour there is the dark of the curl, not the band.
vis&=(AA<=TH_TOP-np.radians(4))|(AA>=TH_BOT+np.radians(4))
T=sample(a,np.clip(XX,0,1023),np.clip(YY,0,1023))
def band_colour(r,th,nearest=False):
    """The band's colour at radius r, carried along the arc from the visible picture: between
    two visible stretches it blends from one to the other, so there is no seam."""
    ri=np.clip(np.rint(r-RI).astype(int),0,len(radii)-1)
    na=len(angs)
    af=((np.degrees(th)+180)/0.25)%na
    out=np.empty(r.shape+(3,))
    for k in np.unique(ri):
        sel=np.nonzero(ri==k)[0]
        v=np.nonzero(vis[k])[0]
        if len(v)==0: out[sel]=T[min(k,len(radii)-1)].mean(0); continue
        q=af[sel]
        # nearest visible index going up and going down (circularly)
        up=(v[None,:]-q[:,None])%na; dn=(q[:,None]-v[None,:])%na
        iu=np.argmin(up,1); idn=np.argmin(dn,1)
        du=up[np.arange(len(q)),iu]; dd=dn[np.arange(len(q)),idn]
        wu=dd/np.maximum(du+dd,1e-9)
        if nearest: wu=(du<dd).astype(float)
        out[sel]=T[k,v[iu]]*wu[:,None]+T[k,v[idn]]*(1-wu[:,None])
    return out
# ---- outside glow, read along the two directions where the band's outer edge is visible
t=np.arange(0,61)
g=[]
for deg in (135,225):
    th=np.radians(deg); g.append(sample(a,cx+(RO+t)*np.cos(th),cy+(RO+t)*np.sin(th)))
G_out=np.mean(g,0)
corner=np.median(np.concatenate([a[:30,:30].reshape(-1,3),a[-30:,:30].reshape(-1,3)]),0)
def glow(d):
    i=np.clip(np.rint(d).astype(int),0,60)
    c=G_out[i]
    far=np.clip((d-60)/80,0,1)[...,None]
    return c*(1-far)+corner*far
# ---- the controller: its silhouette and mirror line
ctl=(a[...,2]>80)&(a[...,2]<215)&(np.hypot(*np.meshgrid(np.arange(1024)-cx,np.arange(1024)-cy))<440)
lab,n=ndimage.label(ctl); s=ndimage.sum(ctl,lab,range(1,n+1)); ctl=ndimage.binary_fill_holes(lab==(np.argmax(s)+1))
ys,xs=np.nonzero(ctl)
best=max(((ctl[ys[(a2-xs>=0)&(a2-xs<1024)],(a2-xs)[(a2-xs>=0)&(a2-xs<1024)]].mean()),a2/2) for a2 in range(1100,1500))
AX=best[1]; print("controller box",xs.min(),xs.max(),ys.min(),ys.max(),"mirror",best)
ctlw=ndimage.gaussian_filter(ndimage.binary_dilation(ctl,iterations=10).astype(float),4)
# ---- where everything goes
x0,x1=cx-RO, max(c[0] for c in caps)+CAP
y0,y1=cy-RO, cy+RO
scale=0.82*1024/max(x1-x0,y1-y0)
ox=512-scale*(x0+x1)/2; oy=512-scale*(y0+y1)/2
print("art box",round(x0),round(x1),round(y0),round(y1),"scale",round(scale,4))
ss=2
X,Y=grid(ss,scale,ox,oy)
inside=(X>=0)&(X<=1023)&(Y>=0)&(Y<=1023)
# How far each point is inside the old picture; within 18 px of its edge the made part and the
# picture are blended, so the join cannot show.
edge=np.minimum.reduce([X,1023-X,Y,1023-Y])
MARGIN=18.0
wpic=np.clip(edge/MARGIN,0,1)
R=np.hypot(X-cx,Y-cy); TH=np.arctan2(Y-cy,X-cx)
arc=(R>=RI)&(R<=RO)&((TH<=TH_TOP)|(TH>=TH_BOT))
reff=R.copy(); incap=np.zeros_like(arc)
for (px,py) in caps:
    d=np.hypot(X-px,Y-py); m=(d<=CAP)&~arc
    incap|=m
    reff[m]=RM+np.sign(R[m]-RM)*d[m]
bandm=arc|incap
pic=sample(a,np.clip(X,0,1023),np.clip(Y,0,1023))
need=wpic<1
model=np.zeros_like(pic)
mb=need&bandm
model[mb]=band_colour(reff[mb],TH[mb])
mc=need&incap
model[mc]=band_colour(reff[mc],TH[mc],nearest=True)
mo=need&~bandm
bx=np.arange(-300,1500); by=np.arange(-300,1400)
BX,BY=np.meshgrid(bx,by)
br=np.hypot(BX-cx,BY-cy); bth=np.arctan2(BY-cy,BX-cx)
braster=(br>=RI)&(br<=RO)&((bth<=TH_TOP)|(bth>=TH_BOT))
for (px,py) in caps: braster|=np.hypot(BX-px,BY-py)<=CAP
dist=ndimage.distance_transform_edt(~braster)
d=dist[np.clip(np.rint(Y[mo]).astype(int)+300,0,len(by)-1),np.clip(np.rint(X[mo]).astype(int)+300,0,len(bx)-1)]
model[mo]=glow(d)
# The open mouth of the C past the old right edge: the picture's own last column carried on,
# except across the controller's rows, where the dark inside is blended from just above it to
# just below it (the controller's grip is mirrored in after).
col=a[:,1023].copy()
c_top,c_bot=ys.min()-12,ys.max()+12
for yy in range(c_top,c_bot+1):
    t=(yy-c_top)/(c_bot-c_top); col[yy]=col[c_top]*(1-t)+col[c_bot]*t
# between the two arms only, fading into the glow further out so no line can show
between=(TH>TH_TOP)&(TH<TH_BOT)
mouth=mo&(X>1023-MARGIN)&between&(Y>212)&(Y<803)
yi=np.clip(Y[mouth],0,1023)
colm=np.stack([np.interp(yi,np.arange(1024),col[:,c]) for c in range(3)],1)
fw=np.clip((X[mouth]-1023)/90,0,1)[:,None]
model[mouth]=colm*(1-fw)+model[mouth]*fw
# the controller's right grip past the old edge, mirrored from its left grip
mg=need&(X>1023-MARGIN)&(X<2*AX-xs.min()+20)
mx=np.clip(2*AX-X[mg],0,1023)
w=ndimage.map_coordinates(ctlw,[np.clip(Y[mg],0,1023),mx],order=1)[:,None]
model[mg]=model[mg]*(1-w)+sample(a,mx,np.clip(Y[mg],0,1023))*w
out=pic*wpic[...,None]+model*(1-wpic[...,None])
# soften only the made part a touch, so its edges match the picture's own softness
res=down(out,ss)
save(res,"Mark-new.png")
