// Run only after both platform artifacts and their signatures have passed verification.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

function writeManifest(directory, version) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw Error('A stable version is required');
  const windows = `CodexPulse_${version}_x64-setup.exe`;
  const mac = `CodexPulse_${version}_universal.app.tar.gz`;
  const dmg = `CodexPulse_${version}_universal.dmg`;
  const platform = (name) => {
    const signature = fs.readFileSync(path.join(directory, name + '.sig'), 'utf8').trim();
    if (!signature || !/^[A-Za-z0-9+/=]+$/.test(signature)) throw Error('Invalid signature');
    if (!fs.statSync(path.join(directory, name)).size) throw Error('Empty artifact');
    return {
      signature,
      url: `https://github.com/lvzixun/CodexPulse/releases/download/v${version}/${name}`,
    };
  };
  const windowsUpdate = platform(windows);
  const macUpdate = platform(mac);
  const manifest = {
    version,
    notes: '应用内自动下载更新，签名验证后点击“重启并更新”。',
    pub_date: new Date().toISOString(),
    platforms: {
      'windows-x86_64': windowsUpdate,
      'darwin-x86_64': macUpdate,
      'darwin-aarch64': macUpdate,
    },
  };
  fs.writeFileSync(path.join(directory, 'latest.json'), JSON.stringify(manifest, null, 2) + '\n');
  const assets = [windows, windows + '.sig', dmg, mac, mac + '.sig', 'latest.json'];
  const sums = assets.map(
    (name) =>
      crypto
        .createHash('sha256')
        .update(fs.readFileSync(path.join(directory, name)))
        .digest('hex') +
      '  ' +
      name,
  );
  fs.writeFileSync(path.join(directory, 'SHA256SUMS.txt'), sums.join('\n') + '\n');
  return manifest;
}
module.exports = { writeManifest };
if (require.main === module) writeManifest(process.argv[2], process.argv[3]);
