import { defineConfig } from 'vite';
import { copyFileSync } from 'node:fs';
export default defineConfig({base:'./', build:{outDir:'../desktop-dist',emptyOutDir:true},plugins:[{name:'synthetic-probe-pages',closeBundle(){for(const file of ['probe.html','probe.js','pilot.js'])copyFileSync(`../desktop-assets/${file}`,`../desktop-dist/${file}`);}}]});
