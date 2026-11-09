import subprocess,collections,pathlib,re
source='OUTPOST Chess Analysis Dashboard.png'
# Square coordinates in the supplied reference. Produce vector paths, not bitmap assets.
squares={'bR':(0,0),'bN':(1,0),'bB':(2,0),'bQ':(3,0),'bK':(6,0),'bP':(0,1),'wR':(0,7),'wN':(2,5),'wB':(3,5),'wQ':(3,7),'wK':(6,7),'wP':(4,4)}
def trace(mask,n):
 pbm=('P1\n%d %d\n'%(n,n)+' '.join('1' if p else '0' for p in mask)).encode()
 svg=subprocess.check_output(['potrace','--svg','--flat','--opttolerance','0.3','--turdsize','2','--output','-'],input=pbm).decode()
 return svg[svg.index('<g '):svg.rindex('</svg>')]
for name,(col,row) in squares.items():
 n=56;x=round(536+col*56.625);y=round(150+row*56.625)
 raw=subprocess.check_output(['magick',source,'-crop',f'{n}x{n}+{x}+{y}','-depth','8','RGB:-'])
 dark=[max(raw[i:i+3])<82 for i in range(0,len(raw),3)]
 for j in range(n*n):
  if j//n<3 or j//n>=n-3 or j%n<3 or j%n>=n-3:dark[j]=False
 # The green recommendation arrow covers the knight's bottom outline in the reference.
 if name=='wN':
  for x in range(22,38):dark[49*n+x]=True
 # Keep the piece's connected contours; discard stray anti-aliasing flecks.
 seen=set();mask=[False]*(n*n)
 for i,v in enumerate(dark):
  if not v or i in seen:continue
  todo=[i];seen.add(i);component=[]
  while todo:
   j=todo.pop();component.append(j)
   for k in [j-n,j+n,j-1 if j%n else -1,j+1 if j%n<n-1 else -1]:
    if 0<=k<n*n and dark[k] and k not in seen:seen.add(k);todo.append(k)
  if len(component)>4:
   for j in component:mask[j]=True
 if name[0]=='b':
  body=trace(mask,n).replace('#000000','#1b211b')
 else:
  # Flood the exterior so the closed contours become a solid ivory silhouette.
  exterior=set();todo=[i for i in range(n*n) if (i<n or i>=n*(n-1) or i%n in [0,n-1]) and not mask[i]]
  exterior.update(todo)
  while todo:
   j=todo.pop()
   for k in [j-n,j+n,j-1 if j%n else -1,j+1 if j%n<n-1 else -1]:
    if 0<=k<n*n and not mask[k] and k not in exterior:exterior.add(k);todo.append(k)
  silhouette=[i not in exterior for i in range(n*n)]
  body=trace(silhouette,n).replace('#000000','#f4f0dc')+trace(mask,n).replace('#000000','#222820')
  if name=='wN':
   # Reconstruct the silhouette obscured by the recommendation arrow.
   body='<path fill="#f4f0dc" d="M23.1 9.2C22.6 10.5 19.9 15 17.1 19.2C11.7 27.4 11 29.9 13.6 32.4C15.5 34.4 20.6 34.5 21.7 32.6L28 30.2C27.5 32 24 35 20.5 40C18 43 17 46 18.5 48L43.5 49C44.5 49 45 47 44 45C42 43.5 43.5 40.5 43.5 34C44 24 41.6 17.7 31 14C29 13.5 26.7 12.8 25.9 11.7C24.8 9.3 24 8 23.1 9.2Z"/>'+trace(mask,n).replace('#000000','#222820')
 text=f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {n} {n}" fill="none">'+body+'</svg>\n'
 pathlib.Path(f'public/pieces/reference/{name}.svg').write_text(text)
