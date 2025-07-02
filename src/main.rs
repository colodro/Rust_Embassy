#![no_std]
#![no_main]

//use cortex_m::Peripherals;
use cortex_m_rt::pre_init;
use core::arch::asm;
use defmt::*;
use embassy_executor::Spawner;
//use embassy_stm32::pac::metadata::Peripheral;
use embassy_stm32::peripherals::ADC1;
use embassy_stm32::time::Hertz;
//use embassy_stm32::rcc::low_level::RccPeripheral;
//use embassy_stm32::timer::low_level::GeneralPurpose16bitInstance;
use embassy_stm32::Config;
use embassy_stm32::adc::{self, Adc, AdcChannel, AnyAdcChannel, SampleTime};
use embassy_stm32::gpio::{Output, Pull, Level, Speed};
use embassy_stm32::interrupt;
//use embassy_stm32::timer::pwm_input::PwmInput;
//use embassy_stm32::time::hz;
//use embassy_stm32::timer::CountingMode;
use embassy_stm32::exti::ExtiInput;
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};


static mut LED_ENABLED: bool = true; // Declare async tasks
#[embassy_executor::task]
async fn button_task(mut button: ExtiInput<'static>) {
    info!("Press the USER button...");

    loop {
        button.wait_for_rising_edge().await;
        info!("Pressed!");
        if button.is_high() {
            info!("Pressed!");
            
            // Inverte o estado do LED e notifica a task principal
            unsafe {
            LED_ENABLED = !LED_ENABLED; // A lterna o estado do LED
        }
            // Espera pela borda de descida (soltura) com debounce
            button.wait_for_falling_edge().await;
            info!("Released!");
        }
    }
} 


//bind_interrupts!(struct Irqs {
//    TIM2 => timer::CaptureCompareInterruptHandler<peripherals::TIM2>;
//});

//#[link_section = ".ram2bss"]

#[link_section = ".ccmram"]
static mut TESTE: i32 = 60;

#[link_section = ".data2"]
static mut TESTE2: i32 = 70;

#[pre_init]
unsafe fn before_main() {
    unsafe {
        asm!{
            "ldr r0, =__sccmdata
            ldr r1, =__eccmdata
            ldr r2, =__siccmdata
            0:
            cmp r1, r0
            beq 1f
            ldm r2!, {{r3}}
            stm r0!, {{r3}}
            b 0b
            1:"
        }

        asm!{
            "ldr r0, =__sdata2
            ldr r1, =__edata2
            ldr r2, =__sidata2
            2:
            cmp r1, r0
            beq 3f
            ldm r2!, {{r3}}
            stm r0!, {{r3}}
            b 2b
            3:"
        }
    }
}


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    unsafe { embassy_stm32::pac::RCC.ahb1enr().modify(|r| r.set_gpiocen(true)); }
    
 
     let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL168,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV7), // USB clock at 48 MHz
            // Main system clock at 168 MHz
            divr: Some(PllRDiv::DIV2),
        });
        config.rcc.sys = Sysclk::PLL1_P;

        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }

    let p: embassy_stm32::Peripherals = embassy_stm32::init(config);

    info!("Hello World!");


    let mut button = ExtiInput::new(p.PA0, p.EXTI0, Pull::Down);



    // Spawned tasks run in the background, concurrently.
    //spawner.spawn(adc_task(adc, p.PA1.degrade_adc())).unwrap();
    spawner.spawn(button_task(button)).unwrap();

    //let mut pwm_input = PwmInput::new(p.TIM2, p.PA0, Pull::None, khz(10));
    //pwm_input.enable();

    let mut led1 = Output::new(p.PD12, Level::High, Speed::Low);
    let mut led2 = Output::new(p.PD13, Level::High, Speed::Low);


     loop {
       unsafe {
            if LED_ENABLED {
                led1.set_high();
                Timer::after_millis(500).await;
                led1.set_low();
                Timer::after_millis(500).await;
            } else {
                // Mantém o LED desligado quando não está piscando
                led1.set_low();
                Timer::after_millis(100).await; // Pequeno delay para não sobrecarregar a CPU
            }
        }
    } 

   
}

