from PIL import Image
import numpy as np
from scipy import ndimage
ASSETS="/projects/sandbox/Continuum-/native/ios/Assets.xcassets/{}.appiconset/icon-1024.png"
def load(n): return np.asarray(Image.open(ASSETS.format(n)).convert("RGB")).astype(np.float64)
def sample(img, xs, ys):
    """Bilinear sample img (H,W,3) at float coords; returns (...,3)."""
    out=np.empty(xs.shape+(3,))
    for c in range(3):
        out[...,c]=ndimage.map_coordinates(img[...,c],[ys,xs],order=1,mode='nearest')
    return out
def grid(ss, scale, ox, oy):
    """Output pixel centres (supersampled ss times) mapped back to source coords.
    Source point s maps to output u = s*scale + o."""
    n=1024*ss
    u=(np.arange(n)+0.5)/ss
    X,Y=np.meshgrid(u,u)
    return (X-ox)/scale, (Y-oy)/scale
def down(img, ss):
    n=img.shape[0]//ss
    return img.reshape(n,ss,n,ss,3).mean((1,3))
def save(arr, path):
    Image.fromarray(np.clip(np.rint(arr),0,255).astype(np.uint8),"RGB").save(path)
def fit_colour(img, mask):
    """Least-squares colour = a + b*x + c*y over mask, returns function(x,y)."""
    ys,xs=np.nonzero(mask)
    A=np.stack([np.ones_like(xs),xs,ys],1).astype(float)
    coef,*_=np.linalg.lstsq(A,img[ys,xs],rcond=None)
    return lambda x,y: coef[0]+np.multiply.outer(x,coef[1])+np.multiply.outer(y,coef[2])

def edge_background(img, art, sigma=24.0, depth=6):
    """The picture's own background along each of its four edges, as smooth 1-D colour strips:
    the outermost rows and columns, with any art on them filled in from the background either
    side, then smoothed. Returns a function giving the background for points off the picture,
    carried straight out from the nearest edge (corners blend the two edges)."""
    from scipy import ndimage as nd
    strips={}
    for side in ("top","bottom","left","right"):
        if side=="top": band=img[:depth]; m=art[:depth].any(0)
        elif side=="bottom": band=img[-depth:]; m=art[-depth:].any(0)
        elif side=="left": band=img[:,:depth].transpose(1,0,2); m=art[:,:depth].any(1)
        else: band=img[:,-depth:].transpose(1,0,2); m=art[:,-depth:].any(1)
        col=np.median(band,0)
        good=np.nonzero(~m)[0]
        idx=np.arange(len(col))
        col=np.stack([np.interp(idx,good,col[good,c]) for c in range(3)],1)
        col=np.stack([nd.gaussian_filter1d(col[:,c],sigma,mode='nearest') for c in range(3)],1)
        strips[side]=col
    def at(x,y):
        xi=np.clip(x,0,1023); yi=np.clip(y,0,1023)
        def s(name,i): 
            c=strips[name]; return np.stack([np.interp(i,np.arange(1024),c[:,k]) for k in range(3)],-1)
        dl=np.maximum(-x,0); dr=np.maximum(x-1023,0); dt=np.maximum(-y,0); db=np.maximum(y-1023,0)
        acc=np.zeros(x.shape+(3,)); w=np.zeros(x.shape)
        for d,name,i in ((dl,"left",yi),(dr,"right",yi),(dt,"top",xi),(db,"bottom",xi)):
            wk=(d>0).astype(float)*(d+1e-3)
            acc+=s(name,i)*wk[...,None]; w+=wk
        return acc/np.maximum(w,1e-9)[...,None]
    return at
