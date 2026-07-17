//! M1 verification: tracking + navigation message decoding.
use gnss_core::acquisition::{PcpsAcquisition, PcpsConfig};
use gnss_core::nav::NavDecoder;
use gnss_core::tracking::{DllPllTracking, TrackingConfig};
use gnss_data::gps_l1ca::{CODE_LENGTH, PRN_CODES};
use num_complex::Complex;
use std::f64::consts::TAU;

const PREAMBLE: u8 = 0x8B;

#[test]
fn test_m1_tracking_locks_on_clean_signal() {
    let fs = 4_000_000.0; let prn = 10; let doppler = 1000.0; let code_phase = 256.0;
    let amp = 5.0; let dur = 500; let spm = fs as usize / 1000; let spc = fs / 1.023e6;
    let code = &PRN_CODES[prn];
    let signal: Vec<Complex<f64>> = (0..dur*spm).map(|i|{
        let n=i%spm; let c=code[((n as f64/spc+code_phase) as usize)%CODE_LENGTH] as f64;
        let p=TAU*doppler*i as f64/fs;
        Complex::new(p.cos(),p.sin())*Complex::new(amp*c,0.0)
    }).collect();
    let cfg=TrackingConfig{fs,..Default::default()};
    let mut trk=DllPllTracking::new(prn,doppler,code_phase,cfg);
    let mut pmax:f64=0.0;
    for ms in 0..dur{let o=trk.process(&signal[ms*spm..(ms+1)*spm]);pmax=pmax.max(o.prompt.norm());}
    assert!(pmax>1000.0,"Prompt too weak: {:.0}",pmax);
}

#[test]
fn test_pll_frequency_pull_in() {
    let fs=4_000_000.0;let prn=10;let true_doppler=1000.0;let init_doppler=850.0;
    let code_phase=256.0;let amp=5.0;let dur=3000;let spm=fs as usize/1000;
    let spc=fs/1.023e6;let code=&PRN_CODES[prn];
    let signal:Vec<Complex<f64>>=(0..dur*spm).map(|i|{
        let n=i%spm;let c=code[((n as f64/spc+code_phase) as usize)%CODE_LENGTH] as f64;
        let p=TAU*true_doppler*i as f64/fs;
        Complex::new(p.cos(),p.sin())*Complex::new(amp*c,0.0)
    }).collect();
    let cfg=TrackingConfig{fs,pll_bw:40.0,dll_bw:4.0,..Default::default()};
    let mut trk=DllPllTracking::new(prn,init_doppler,code_phase,cfg);
    let mut f=0.0;
    for ms in 0..dur{
        let o=trk.process(&signal[ms*spm..(ms+1)*spm]);f=o.carrier_doppler_hz;
        if ms%500==0{println!("ms={}: doppler={:.1}, I={:.1}, Q={:.1}",ms,f,o.prompt.re,o.prompt.im);}
    }
    let e=(f-true_doppler).abs();
    println!("Final: {:.1}, True: {:.1}, Error: {:.1}Hz",f,true_doppler,e);
    assert!(e<50.0,"PLL pull-in error: {:.1}Hz",e);
}

fn make_tlm(ds:u8,de:u8)->u32{let mut w:u32=(PREAMBLE as u32)<<22;let d=|n:u32|->u8{((w>>(30-n))&1)as u8};let p25=ds^d(1)^d(2)^d(3)^d(5)^d(6)^d(10)^d(11)^d(12)^d(13)^d(14)^d(17)^d(18)^d(20)^d(23);let p26=de^d(2)^d(3)^d(4)^d(6)^d(7)^d(11)^d(12)^d(13)^d(14)^d(15)^d(18)^d(19)^d(21)^d(24);let p27=ds^d(1)^d(3)^d(4)^d(5)^d(7)^d(8)^d(12)^d(13)^d(14)^d(15)^d(16)^d(19)^d(20)^d(22);let p28=de^d(2)^d(4)^d(5)^d(6)^d(8)^d(9)^d(13)^d(14)^d(15)^d(16)^d(17)^d(20)^d(21)^d(23);let p29=de^d(1)^d(3)^d(5)^d(6)^d(7)^d(9)^d(10)^d(14)^d(15)^d(16)^d(17)^d(18)^d(21)^d(22)^d(24);let p30=ds^d(3)^d(5)^d(6)^d(8)^d(9)^d(10)^d(11)^d(13)^d(15)^d(19)^d(22)^d(23)^d(24);w|(p25 as u32)<<5|(p26 as u32)<<4|(p27 as u32)<<3|(p28 as u32)<<2|(p29 as u32)<<1|p30 as u32}

fn make_how(tow:u32,sf:u8,ds:u8,de:u8)->u32{let mut w:u32=0;w|=(tow&0x1FFFF)<<13;w|=((sf&0x7)as u32)<<8;let d=|n:u32|->u8{((w>>(30-n))&1)as u8};let p25=ds^d(1)^d(2)^d(3)^d(5)^d(6)^d(10)^d(11)^d(12)^d(13)^d(14)^d(17)^d(18)^d(20)^d(23);let p26=de^d(2)^d(3)^d(4)^d(6)^d(7)^d(11)^d(12)^d(13)^d(14)^d(15)^d(18)^d(19)^d(21)^d(24);let p27=ds^d(1)^d(3)^d(4)^d(5)^d(7)^d(8)^d(12)^d(13)^d(14)^d(15)^d(16)^d(19)^d(20)^d(22);let p28=de^d(2)^d(4)^d(5)^d(6)^d(8)^d(9)^d(13)^d(14)^d(15)^d(16)^d(17)^d(20)^d(21)^d(23);let p29=de^d(1)^d(3)^d(5)^d(6)^d(7)^d(9)^d(10)^d(14)^d(15)^d(16)^d(17)^d(18)^d(21)^d(22)^d(24);let p30=ds^d(3)^d(5)^d(6)^d(8)^d(9)^d(10)^d(11)^d(13)^d(15)^d(19)^d(22)^d(23)^d(24);w|(p25 as u32)<<5|(p26 as u32)<<4|(p27 as u32)<<3|(p28 as u32)<<2|(p29 as u32)<<1|p30 as u32}

fn wb(w:u32)->Vec<u8>{(0..30).rev().map(|i|((w>>i)&1)as u8).collect()}

#[test]
fn test_m1_track_and_decode_subframe() {
    let fs=4_000_000.0;let prn=10;let doppler=1000.0;let cp=256.0;let amp=3.0;
    let spm=fs as usize/1000;let spc=fs/1.023e6;let code=&PRN_CODES[prn];
    let tlm=make_tlm(0,0);let td29=((tlm>>1)&1)as u8;let td30=(tlm&1)as u8;
    let how=make_how(100,1,td29,td30);
    let mut bits=wb(tlm);bits.extend(&wb(how));
    for _ in 0..8{bits.extend(&wb(0));}
    let dur=bits.len()*20;
    let signal:Vec<Complex<f64>>=(0..dur*spm).map(|i|{
        let n=i%spm;let ms=i/spm;let bi=ms/20;let b=if bi<bits.len(){bits[bi]}else{0};
        let bm=if b==1{1.0}else{-1.0};
        let ci=((n as f64/spc+cp)as usize)%CODE_LENGTH;let c=code[ci]as f64;
        let p=TAU*doppler*i as f64/fs;
        Complex::new(p.cos(),p.sin())*Complex::new(amp*c*bm,0.0)
    }).collect();
    let acq=PcpsConfig{fs,doppler_max:5000.0,doppler_step:500.0,threshold:3.0,..Default::default()};
    let mut a=PcpsAcquisition::new(acq);let ar=a.acquire(prn,&signal[..spm]);
    assert!(ar.detected);println!("Acq: d={:.0}, cp={:.1}",ar.doppler_hz,ar.code_phase_chips);
    let tc=TrackingConfig{fs,..Default::default()};
    let mut trk=DllPllTracking::new(prn,ar.doppler_hz,ar.code_phase_chips,tc);
    let mut dec=NavDecoder::new();let mut ok=false;
    for ms in 0..dur{
        let o=trk.process(&signal[ms*spm..(ms+1)*spm]);
        if let Some(sym)=trk.take_symbol(){
            let bit=if sym.re>0.0{1}else{0};
            if dec.feed_bit(bit){ok=true;println!("SF={}, TOW={}",dec.subframe_id,dec.tow);
                assert_eq!(dec.subframe_id,1);assert_eq!(dec.tow,100);break;}
        }
    }
    assert!(ok,"Failed to decode subframe");
}
