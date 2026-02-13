import React from 'react';
import { SafeAreaView, StatusBar, StyleSheet, Text, View, Platform, NativeModules, ScrollView, TouchableOpacity, Alert } from 'react-native';

// Import the native module directly
const { LedgerFFI } = NativeModules;

interface HashResult {
  type: string;
  data: string;
  size: number;
}

interface DemoSection {
  title: string;
  description: string;
  functions: DemoFunction[];
}

interface DemoFunction {
  name: string;
  description: string;
  test: () => Promise<any>;
  result: any;
  loading: boolean;
  error: string | null;
}

export default function App() {
  const [sections, setSections] = React.useState<DemoSection[]>([]);
  const [overallError, setOverallError] = React.useState<string | null>(null);
  const [isRunningAll, setIsRunningAll] = React.useState<boolean>(false);

  const createDemoSections = (): DemoSection[] => [
    {
      title: "Basic Functions",
      description: "Core ledger functionality",
      functions: [
        {
          name: "hello",
          description: "Basic hello function to test module connectivity",
          test: async () => await LedgerFFI.hello(),
          result: null,
          loading: false,
          error: null
        }
      ]
    },
    {
      title: "Token Types",
      description: "Different token type definitions",
      functions: [
        {
          name: "nativeToken",
          description: "Get native token type",
          test: async () => await LedgerFFI.nativeToken(),
          result: null,
          loading: false,
          error: null
        },
        {
          name: "feeToken",
          description: "Get fee token type",
          test: async () => await LedgerFFI.feeToken(),
          result: null,
          loading: false,
          error: null
        },
        {
          name: "shieldedToken",
          description: "Get shielded token type",
          test: async () => await LedgerFFI.shieldedToken(),
          result: null,
          loading: false,
          error: null
        },
        {
          name: "unshieldedToken",
          description: "Get unshielded token type",
          test: async () => await LedgerFFI.unshieldedToken(),
          result: null,
          loading: false,
          error: null
        }
      ]
    },
    {
      title: "Sample Data",
      description: "Generate sample cryptographic data",
      functions: [
        {
          name: "sampleCoinPublicKey",
          description: "Generate sample coin public key",
          test: async () => await LedgerFFI.sampleCoinPublicKey(),
          result: null,
          loading: false,
          error: null
        },
        {
          name: "sampleEncryptionPublicKey",
          description: "Generate sample encryption public key",
          test: async () => await LedgerFFI.sampleEncryptionPublicKey(),
          result: null,
          loading: false,
          error: null
        },
        {
          name: "sampleIntentHash",
          description: "Generate sample intent hash",
          test: async () => await LedgerFFI.sampleIntentHash(),
          result: null,
          loading: false,
          error: null
        }
      ]
    },
    {
      title: "Type Conversion",
      description: "Convert between different data formats",
      functions: [
        {
          name: "shieldedTokenTypeFromBytes",
          description: "Convert bytes to shielded token type",
          test: async () => {
            // Use sample data from another function
            const sampleBytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32];
            return await LedgerFFI.shieldedTokenTypeFromBytes(sampleBytes);
          },
          result: null,
          loading: false,
          error: null
        },
        {
          name: "publicKeyFromBytes",
          description: "Convert bytes to public key",
          test: async () => {
            const sampleBytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32];
            return await LedgerFFI.publicKeyFromBytes(sampleBytes);
          },
          result: null,
          loading: false,
          error: null
        },
        {
          name: "userAddressFromBytes",
          description: "Convert bytes to user address",
          test: async () => {
            const sampleBytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32];
            return await LedgerFFI.userAddressFromBytes(sampleBytes);
          },
          result: null,
          loading: false,
          error: null
        }
      ]
    },
    {
      title: "Cryptographic Functions",
      description: "Advanced cryptographic operations",
      functions: [
        {
          name: "addressFromKey",
          description: "Generate address from key",
          test: async () => {
            const sampleKey = "sample_key_for_testing_purposes_only";
            return await LedgerFFI.addressFromKey(sampleKey);
          },
          result: null,
          loading: false,
          error: null
        }
      ]
    }
  ];

  const testFunction = async (sectionIndex: number, functionIndex: number) => {
    if (!LedgerFFI) {
      Alert.alert('Error', 'LedgerFFI module is not available');
      return;
    }

    setSections(prev => {
      const newSections = [...prev];
      newSections[sectionIndex].functions[functionIndex].loading = true;
      newSections[sectionIndex].functions[functionIndex].error = null;
      return newSections;
    });

    try {
      const result = await sections[sectionIndex].functions[functionIndex].test();
      setSections(prev => {
        const newSections = [...prev];
        newSections[sectionIndex].functions[functionIndex].result = result;
        newSections[sectionIndex].functions[functionIndex].loading = false;
        return newSections;
      });
    } catch (error: any) {
      setSections(prev => {
        const newSections = [...prev];
        newSections[sectionIndex].functions[functionIndex].error = error?.message || String(error);
        newSections[sectionIndex].functions[functionIndex].loading = false;
        return newSections;
      });
    }
  };

  const testAllFunctions = async () => {
    if (!LedgerFFI) {
      Alert.alert('Error', 'LedgerFFI module is not available');
      return;
    }

    setIsRunningAll(true);
    setOverallError(null);

    for (let sectionIndex = 0; sectionIndex < sections.length; sectionIndex++) {
      for (let functionIndex = 0; functionIndex < sections[sectionIndex].functions.length; functionIndex++) {
        await testFunction(sectionIndex, functionIndex);
        // Small delay to show progress
        await new Promise(resolve => setTimeout(resolve, 100));
      }
    }

    setIsRunningAll(false);
  };

  const testSection = async (sectionIndex: number) => {
    if (!LedgerFFI) {
      Alert.alert('Error', 'LedgerFFI module is not available');
      return;
    }

    for (let functionIndex = 0; functionIndex < sections[sectionIndex].functions.length; functionIndex++) {
      await testFunction(sectionIndex, functionIndex);
      await new Promise(resolve => setTimeout(resolve, 50));
    }
  };

  React.useEffect(() => {
    setSections(createDemoSections());
  }, []);

  const renderFunctionResult = (func: DemoFunction) => {
    if (func.loading) {
      return (
        <View style={styles.loadingContainer}>
          <Text style={styles.loadingText}>Testing...</Text>
        </View>
      );
    }

    if (func.error) {
      return (
        <View style={styles.errorContainer}>
          <Text style={styles.errorText}>Error: {func.error}</Text>
        </View>
      );
    }

    if (func.result !== null) {
      return (
        <View style={styles.resultContainer}>
          <Text style={styles.resultText}>
            {typeof func.result === 'string' 
              ? func.result 
              : JSON.stringify(func.result, null, 2)
            }
          </Text>
        </View>
      );
    }

    return null;
  };

  const renderSection = (section: DemoSection, sectionIndex: number) => (
    <View key={sectionIndex} style={styles.section}>
      <View style={styles.sectionHeader}>
        <Text style={styles.sectionTitle}>{section.title}</Text>
        <Text style={styles.sectionDescription}>{section.description}</Text>
        <TouchableOpacity 
          style={styles.sectionButton} 
          onPress={() => testSection(sectionIndex)}
        >
          <Text style={styles.sectionButtonText}>Test Section</Text>
        </TouchableOpacity>
      </View>
      
      {section.functions.map((func, funcIndex) => (
        <View key={funcIndex} style={styles.functionCard}>
          <View style={styles.functionHeader}>
            <Text style={styles.functionName}>{func.name}</Text>
            <TouchableOpacity 
              style={styles.functionButton} 
              onPress={() => testFunction(sectionIndex, funcIndex)}
              disabled={func.loading}
            >
              <Text style={styles.functionButtonText}>
                {func.loading ? 'Testing...' : 'Test'}
              </Text>
            </TouchableOpacity>
          </View>
          <Text style={styles.functionDescription}>{func.description}</Text>
          {renderFunctionResult(func)}
        </View>
      ))}
    </View>
  );

  return (
    <SafeAreaView style={styles.container}>
      <StatusBar barStyle={Platform.OS === 'ios' ? 'dark-content' : 'light-content'} />
      <ScrollView style={styles.scrollView}>
        <View style={styles.header}>
          <Text style={styles.title}>React Native Ledger FFI Demo</Text>
          <Text style={styles.subtitle}>Comprehensive API Testing</Text>
          <TouchableOpacity 
            style={styles.mainButton} 
            onPress={testAllFunctions} 
            disabled={isRunningAll}
          >
            <Text style={styles.mainButtonText}>
              {isRunningAll ? 'Running All Tests...' : 'Test All Functions'}
            </Text>
          </TouchableOpacity>
        </View>

        {sections.map((section, index) => renderSection(section, index))}

        {overallError ? (
          <View style={styles.overallErrorCard}>
            <Text style={styles.overallErrorTitle}>Overall Error:</Text>
            <Text style={styles.overallErrorText}>{overallError}</Text>
          </View>
        ) : null}
      </ScrollView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#0b0f17'
  },
  scrollView: {
    flex: 1,
  },
  header: {
    alignItems: 'center',
    padding: 20,
    backgroundColor: '#121826',
    marginBottom: 16
  },
  title: {
    color: '#fff',
    fontSize: 24,
    fontWeight: '700',
    marginBottom: 8
  },
  subtitle: {
    color: '#9fb3c8',
    fontSize: 16,
    marginBottom: 16
  },
  mainButton: {
    backgroundColor: '#4ade80',
    paddingHorizontal: 24,
    paddingVertical: 12,
    borderRadius: 8,
    marginTop: 8
  },
  mainButtonText: {
    color: '#000',
    fontSize: 16,
    fontWeight: '600'
  },
  section: {
    margin: 16,
    marginBottom: 8
  },
  sectionHeader: {
    backgroundColor: '#1e293b',
    borderRadius: 12,
    padding: 16,
    marginBottom: 8,
    borderLeftWidth: 4,
    borderLeftColor: '#3b82f6'
  },
  sectionTitle: {
    color: '#fff',
    fontSize: 20,
    fontWeight: '600',
    marginBottom: 4
  },
  sectionDescription: {
    color: '#9fb3c8',
    fontSize: 14,
    marginBottom: 12
  },
  sectionButton: {
    backgroundColor: '#3b82f6',
    paddingHorizontal: 16,
    paddingVertical: 8,
    borderRadius: 6,
    alignSelf: 'flex-start'
  },
  sectionButtonText: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '500'
  },
  functionCard: {
    backgroundColor: '#121826',
    borderRadius: 8,
    padding: 16,
    marginBottom: 8,
    borderLeftWidth: 2,
    borderLeftColor: '#4ade80'
  },
  functionHeader: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 8
  },
  functionName: {
    color: '#fff',
    fontSize: 16,
    fontWeight: '600',
    flex: 1
  },
  functionButton: {
    backgroundColor: '#4ade80',
    paddingHorizontal: 12,
    paddingVertical: 6,
    borderRadius: 4
  },
  functionButtonText: {
    color: '#000',
    fontSize: 12,
    fontWeight: '500'
  },
  functionDescription: {
    color: '#9fb3c8',
    fontSize: 14,
    marginBottom: 8
  },
  loadingContainer: {
    backgroundColor: '#1e293b',
    padding: 12,
    borderRadius: 6,
    marginTop: 8
  },
  loadingText: {
    color: '#fbbf24',
    fontSize: 14,
    textAlign: 'center'
  },
  errorContainer: {
    backgroundColor: '#7f1d1d',
    padding: 12,
    borderRadius: 6,
    marginTop: 8
  },
  errorText: {
    color: '#fca5a5',
    fontSize: 12
  },
  resultContainer: {
    backgroundColor: '#0f172a',
    padding: 12,
    borderRadius: 6,
    marginTop: 8
  },
  resultText: {
    color: '#4ade80',
    fontSize: 12,
    fontFamily: 'monospace'
  },
  overallErrorCard: {
    backgroundColor: '#7f1d1d',
    borderRadius: 8,
    padding: 16,
    margin: 16,
    borderLeftWidth: 4,
    borderLeftColor: '#f87171'
  },
  overallErrorTitle: {
    color: '#f87171',
    fontSize: 16,
    fontWeight: '600',
    marginBottom: 8
  },
  overallErrorText: {
    color: '#fca5a5',
    fontSize: 14
  }
});