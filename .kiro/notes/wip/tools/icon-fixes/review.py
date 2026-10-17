import sys
from PIL import Image, ImageDraw
def masked(im, S):
    im=im.convert("RGB").resize((S,S),Image.LANCZOS)
    m=Image.new("L",(S*4,S*4),0); ImageDraw.Draw(m).rounded_rectangle([0,0,S*4-1,S*4-1],radius=int(S*4*0.2237),fill=255)
    m=m.resize((S,S),Image.LANCZOS)
    bg=Image.new("RGB",(S,S),(28,28,30)); bg.paste(im,(0,0),m); return bg
def sheet(pairs, out, S=300):
    W=len(pairs)
    sh=Image.new("RGB",(W*(2*S+30)+10, S+20),(28,28,30))
    for i,(old,new) in enumerate(pairs):
        x=10+i*(2*S+30)
        sh.paste(masked(Image.open(old),S),(x,10)); sh.paste(masked(Image.open(new),S),(x+S+10,10))
    sh.save(out)
if __name__=="__main__":
    args=sys.argv[2:]
    sheet([(args[i],args[i+1]) for i in range(0,len(args),2)], sys.argv[1])
