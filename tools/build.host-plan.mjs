import { execFileSync } from 'node:child_process';
import { join, isAbsolute } from 'node:path';
export function cargoTargetDirectory(crateDirectory, env=process.env) {
  const metadata=JSON.parse(execFileSync('cargo',['metadata','--format-version','1','--no-deps','--locked'],{cwd:crateDirectory,env,encoding:'utf8'}));
  return metadata.target_directory;
}
export function hostBuildPlan(targetDirectory, compilerVersion, productName) {
  if(typeof targetDirectory!=='string' || !isAbsolute(targetDirectory))throw new Error('Cargo metadata must provide an absolute target_directory');
  const targetTriple=compilerVersion.match(/^host: (.+)$/m)?.[1];
  if(!['aarch64-apple-darwin','x86_64-apple-darwin'].includes(targetTriple))throw new Error('macOS internal packaging requires a native macOS Rust host compiler');
  return {targetDirectory,targetTriple,targetArguments:['--target',targetTriple],bundlePath:join(targetDirectory,targetTriple,'release/bundle/macos',`${productName}.app`)};
}
// NUL-delimited shell plan: paths with spaces remain single arguments. Resolving
// this plan reads metadata only; it never compiles, signs, installs or launches.
if(import.meta.main) {
  const [crate,product]=process.argv.slice(2);
  if(!crate || !product)throw new Error('Usage: bun tools/build.host-plan.mjs <Host crate> <product name>');
  const plan=hostBuildPlan(cargoTargetDirectory(crate),execFileSync('rustc',['-vV'],{encoding:'utf8'}),product);
  process.stdout.write([plan.targetDirectory,plan.bundlePath,'--config','src-tauri/tauri.macos.conf.json','--bundles','app',...plan.targetArguments,'--','--locked'].join('\0'));
}
