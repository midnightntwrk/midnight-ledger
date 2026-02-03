import { Redirect } from 'expo-router';
import { useWallet } from '../src/providers';

export default function Index() {
  const { status } = useWallet();

  if (status === 'connected') {
    return <Redirect href="/(tabs)" />;
  }

  return <Redirect href="/(auth)/welcome" />;
}
