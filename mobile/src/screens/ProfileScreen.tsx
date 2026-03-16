import React from 'react';
import {StyleSheet, Text, TouchableOpacity, View} from 'react-native';
import {SafeAreaView} from 'react-native-safe-area-context';
import {useAuth} from '../context/AuthContext';
import {C, T} from '../theme';

export function ProfileScreen() {
  const {isConnected, address} = useAuth();
  const short = address ? `${address.slice(0, 6)}…${address.slice(-4)}` : null;

  return (
    <SafeAreaView style={styles.container} edges={['top']}>
      <View style={styles.header}>
        <Text style={styles.title}>Profile</Text>
      </View>
      {isConnected && address ? (
        <View style={styles.profile}>
          <View style={styles.avatar}>
            <Text style={styles.avTxt}>{address.slice(2, 4).toUpperCase()}</Text>
          </View>
          <Text style={styles.addr}>{short}</Text>
          <View style={styles.stats}>
            {[['Posts', '0'], ['Following', '0'], ['Followers', '0'], ['YEET', '0']].map(([l, v]) => (
              <View key={l} style={styles.stat}>
                <Text style={styles.statV}>{v}</Text>
                <Text style={styles.statL}>{l}</Text>
              </View>
            ))}
          </View>
        </View>
      ) : (
        <View style={styles.empty}>
          <Text style={styles.emptyTxt}>Connect your wallet to view your profile.</Text>
        </View>
      )}
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {flex: 1, backgroundColor: C.bg},
  header: {paddingHorizontal: 16, paddingVertical: 14, borderBottomWidth: 1, borderBottomColor: C.line},
  title: {color: '#eef0f6', fontFamily: T.mono, fontSize: 14, fontWeight: '500'},
  profile: {alignItems: 'center', paddingTop: 32, paddingHorizontal: 16},
  avatar: {
    width: 72, height: 72, backgroundColor: C.panel,
    borderWidth: 2, borderColor: C.accent,
    alignItems: 'center', justifyContent: 'center', marginBottom: 12,
  },
  avTxt: {color: C.accent, fontFamily: T.mono, fontSize: 22, fontWeight: '500'},
  addr: {color: C.accent, fontFamily: T.mono, fontSize: 13, marginBottom: 24},
  stats: {flexDirection: 'row', gap: 0, width: '100%', borderWidth: 1, borderColor: C.line},
  stat: {flex: 1, alignItems: 'center', paddingVertical: 14, borderRightWidth: 1, borderRightColor: C.line},
  statV: {color: '#eef0f6', fontFamily: T.mono, fontSize: 16, fontWeight: '500'},
  statL: {color: C.muted, fontFamily: T.mono, fontSize: 9, textTransform: 'uppercase', letterSpacing: 0.8, marginTop: 2},
  empty: {flex: 1, alignItems: 'center', justifyContent: 'center'},
  emptyTxt: {color: C.muted, fontFamily: T.mono, fontSize: 12, textAlign: 'center'},
});