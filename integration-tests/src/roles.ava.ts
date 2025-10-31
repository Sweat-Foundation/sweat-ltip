import { createTest } from './setup.ts';

const test = createTest();

test('Check roles', async t => {
  console.log('🧪 Check roles');

  const { contract, issuer, executor } = t.context.accounts;

  let issuers: Array<string> = await contract.view('members', { role: 'issuer' });

  t.is(issuers.length, 1);
  t.is(issuers.at(0), issuer.accountId);

  let executors: Array<string> = await contract.view('members', { role: 'executor' });

  t.is(executors.length, 1);
  t.is(executors.at(0), executor.accountId);
});
