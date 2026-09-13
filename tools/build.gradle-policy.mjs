import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { root } from './doctor.mjs';
const dir=await mkdtemp(join(tmpdir(),'leftcar-gradle-policy-'));
await writeFile(join(dir,'settings.gradle'), "rootProject.name = 'leftcar-policy-fixture'\n");
await writeFile(join(dir,'build.gradle'), `apply from: ${JSON.stringify(join(root,'tools/build.android.gradle'))}
 tasks.register('verifyPolicy') { doLast {
   assert leftcarProfile(null).suffix == ''
   assert leftcarProfile(null).scheme == 'leftcar'
   assert leftcarProfile(null).accessory == 'android.hardware.usb.action.USB_ACCESSORY_ATTACHED'
   def profile = leftcarProfile('baseline-068')
   assert profile.suffix == '.benchmark.baseline_068'
   assert profile.scheme != 'leftcar'
   assert profile.expoScheme != 'exp+leftcar-viewer'
   assert profile.accessory != 'android.hardware.usb.action.USB_ACCESSORY_ATTACHED'
   ['', '../bad', 'UPPER', 'bad.name'].each { slug ->
     try { leftcarProfile(slug); assert false : 'invalid profile accepted' } catch (GradleException expected) {}
   }
   assert leftcarCargoTarget(new File('/repo'), null).path == '/repo/target'
   assert leftcarCargoTarget(new File('/repo'), '/alternate').path == '/alternate'
   assert leftcarCargoTarget(new File('/repo'), 'alternate').path == '/repo/alternate'
   assert leftcarDebugKeystore(new File('/app'), null).path == '/app/debug.keystore'
   try { leftcarDebugKeystore(new File('/app'), 'relative.keystore'); assert false : 'relative override accepted' } catch (GradleException expected) {}
   try { leftcarDebugKeystore(new File('/app'), '/missing/debug.keystore'); assert false : 'missing override accepted' } catch (GradleException expected) {}
   assert leftcarReleaseSigning(true, [:]) == 'internal-debug-key'
   try { leftcarReleaseSigning(false, [:]); assert false : 'unsigned release accepted' } catch (GradleException expected) {}
   println 'PASS profile handlers, Cargo target directory, explicit internal signing and missing-public-signing rejection'
 } }
`);
const result=spawnSync(join(root,'apps/viewer-expo/android',process.platform==='win32'?'gradlew.bat':'gradlew'),['-p',dir,'verifyPolicy','--no-daemon'],{cwd:root,stdio:'inherit'});
if(result.error||result.status!==0)throw new Error(`Gradle policy fixture failed (${result.status})`);
console.log(`Actual Gradle fixture: ${dir}`);
