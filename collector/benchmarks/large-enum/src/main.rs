enum Thing {
  Case1(u32, u32),
  Case2(u32, u32),
  Case3(u32, u32),
  Case4(u32, u32),
  Case5(u32, u32),
  Case6(u32, u32),
  Case7(u32, u32),
  Case8(u32, u32),
  Case9(u32, u32),
  Case10(u32, u32),
  Case11(u32, u32),
  Case12(u32, u32),
  Case13(u32, u32),
  Case14(u32, u32),
  Case15(u32, u32),
  Case16(u32, u32),
  Case17(u32, u32),
  Case18(u32, u32),
  Case19(u32, u32),
  Case20(u32, u32),
  Case21(u32, u32),
  Case22(u32, u32),
  Case23(u32, u32),
  Case24(u32, u32),
  Case25(u32, u32),
  Case26(u32, u32),
  Case27(u32, u32),
  Case28(u32, u32),
  Case29(u32, u32),
  Case30(u32, u32) 
}

fn go(thing: &Thing) -> u32 {
    match thing {
        Thing::Case1(value, _) => *value,
        _ => 5
    }
}

fn main() {

}

