import _ from 'lodash';
import fs from 'fs';

const features = process.argv.slice(2);
const data = _.template(fs.readFileSync('ledger-v6.template.d.ts', 'utf8'), { imports: { fs } })({ features });
fs.writeFileSync('ledger-v6.d.ts', data, 'utf8');
