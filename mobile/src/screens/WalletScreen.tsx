import React, {useState} from 'react';
import {
  Alert, StyleSheet, Text, TouchableOpacity, View,
} from 'react-native';
import {SafeAreaView} from 'react-native-safe-area-context';
import {useAuth} from '../context/AuthContext';
import {C, T} from '../theme';

export function WalletScreen() {
  const {isConnected, address, disconnect} = useAuth();

  if (isConnected && address) {
    return (
      <SafeAreaView style={styles.container} edges={['top']}>
        <View style={styles.header}>
          <Text style={styles.title}>Wallet</Text>
        </View>
        <View style={styles.card}>
          <View style={styles.connected}>
            <View style={[styles.dot, {backgroundColor: C.accent}]} />
            <Text style={styles.connTxt}>Connected</Text>
          </View>
          <Text style={styles.addr}>{address.slice(0, 10)}…{address.slice(-8)}</Text>
          <View style={styles.balance}>
            <Text style={styles.balLabel}>YEET Balance</Text>
            <Text style={styles.balAmt}>0 YEET</Text>
          </View>
          <View style={styles.balance}>
            <Text style={styles.balLabel}>BNB Balance</Text>
            <Text style={styles.balAmt}>— BNB</Text>
          </View>
          <TouchableOpacity
            style={styles.disconnectBtn}
            onPress={() => Alert.alert('Disconnect', 'Disconnect wallet?', [
              {text: 'Cancel'},
              {text: 'Disconnect', style: 'destructive', onPress: disconnect},
            ])}>
            <Text style={styles.disconnectTxt}>Disconnect</Text>
          </TouchableOpacity>
        </View>
        <View style={styles.info}>
          <Text style={styles.infoTxt}>Token tipping and NFT features coming soon.</Text>
        </View>
      </SafeAreaView>
    );
  }

  return (
    <SafeAreaView style={styles.container} edges={['top']}>
      <View style={styles.header}>
        <Text style={styles.title}>Wallet</Text>
      </View>
      <View style={styles.card}>
        <Text style={styles.headline}>Connect your BNB Smart Chain Wallet</Text>
        <Text style={styles.sub}>
          Sign in with MetaMask or WalletConnect to post, earn YEET tokens, and tip creators.
        </Text>
        <View style={styles.steps}>
          {[
            ['1', 'Get a one-time nonce from the server'],
            ['2', 'Sign the message — no gas fee'],
            ['3', 'Your wallet IS your account'],
          ].map(([n, t]) => (
            <View key={n} style={styles.step}>
              <View style={styles.stepNum}>
                <Text style={styles.stepN}>{n}</Text>
              </View>
              <Text style={styles.stepTxt}>{t}</Text>
            </View>
          ))}
        </View>
        <TouchableOpacity style={styles.connectBtn}>
          <Text style={styles.connectTxt}>🦊 Open in Browser to Connect</Text>
        </TouchableOpacity>
        <Text style={styles.note}>
          WalletConnect deep-link support coming in v0.2
        </Text>
      </View>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {flex: 1, backgroundColor: C.bg},
  header: {
    paddingHorizontal: 16, paddingVertical: 14,
    borderBottomWidth: 1, borderBottomColor: C.line,
  },
  title: {color: '#eef0f6', fontFamily: T.mono, fontSize: 14, fontWeight: '500'},
  card: {margin: 16, backgroundColor: C.surface, borderWidth: 1, borderColor: C.line2, padding: 20},
  connected: {flexDirection: 'row', alignItems: 'center', gap: 8, marginBottom: 8},
  dot: {width: 8, height: 8, borderRadius: 4},
  connTxt: {color: C.accent, fontFamily: T.mono, fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.8},
  addr: {color: C.accent, fontFamily: T.mono, fontSize: 13, fontWeight: '500', marginBottom: 16},
  balance: {flexDirection: 'row', justifyContent: 'space-between', paddingVertical: 10, borderBottomWidth: 1, borderBottomColor: C.line},
  balLabel: {color: C.muted, fontFamily: T.mono, fontSize: 12},
  balAmt: {color: '#eef0f6', fontFamily: T.mono, fontSize: 12, fontWeight: '500'},
  disconnectBtn: {marginTop: 16, borderWidth: 1, borderColor: C.red + '66', padding: 12, alignItems: 'center'},
  disconnectTxt: {color: C.red, fontFamily: T.mono, fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.8},
  headline: {color: '#eef0f6', fontFamily: T.mono, fontSize: 15, fontWeight: '500', marginBottom: 8},
  sub: {color: C.muted, fontFamily: T.mono, fontSize: 12, lineHeight: 18, marginBottom: 20},
  steps: {gap: 12, marginBottom: 20},
  step: {flexDirection: 'row', alignItems: 'flex-start', gap: 12},
  stepNum: {width: 22, height: 22, backgroundColor: C.accent, alignItems: 'center', justifyContent: 'center', flexShrink: 0},
  stepN: {color: '#060801', fontFamily: T.mono, fontSize: 11, fontWeight: '700'},
  stepTxt: {color: '#d0d4e0', fontFamily: T.mono, fontSize: 12, flex: 1, lineHeight: 18},
  connectBtn: {backgroundColor: C.accent, padding: 14, alignItems: 'center'},
  connectTxt: {color: '#060801', fontFamily: T.mono, fontSize: 12, fontWeight: '500'},
  note: {color: C.muted, fontFamily: T.mono, fontSize: 10, textAlign: 'center', marginTop: 12},
  info: {padding: 16},
  infoTxt: {color: C.muted, fontFamily: T.mono, fontSize: 11, textAlign: 'center'},
});