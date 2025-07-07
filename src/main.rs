#![no_std]
#![no_main]

use cortex_m_rt::pre_init;
use core::arch::asm;
use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::peripherals::ADC1;
use embassy_stm32::time::Hertz;
use embassy_stm32::Config;
use embassy_stm32::adc::{self, Adc, AdcChannel, AnyAdcChannel, SampleTime};
use embassy_stm32::gpio::{Output, Pull, Level, Speed};
use embassy_stm32::interrupt;
use embassy_stm32::exti::ExtiInput;
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};
use embassy_stm32::bind_interrupts;
use embassy_stm32::usart::{self, Uart};
use heapless::String;

static mut LED_ENABLED: bool = true;

#[embassy_executor::task]
async fn adc_task(mut adc: adc::Adc<'static, ADC1>, mut adc_pin: AnyAdcChannel<ADC1>) {
    adc.set_sample_time(SampleTime::CYCLES144);

    loop {
        let measured = adc.blocking_read(&mut adc_pin);
        info!("measured: {}", measured);
        Timer::after_millis(500).await;
    }
}

bind_interrupts!(struct Irqs {
    USART1 => embassy_stm32::usart::InterruptHandler<embassy_stm32::peripherals::USART1>;
});

// Estrutura para armazenar o comando
struct ShellCommand {
    buffer: String<64>,
    ready: bool,
}

impl ShellCommand {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            ready: false,
        }
    }

    fn add_char(&mut self, c: char) {
        if c == '\r' || c == '\n' {
            self.ready = true;
        } else if c == '\x08' || c == '\x7f' { // Backspace
            self.buffer.pop();
        } else if self.buffer.len() < 63 {
            let _ = self.buffer.push(c);
        }
    }

    fn get_command(&mut self) -> Option<String<64>> {
        if self.ready {
            let cmd = self.buffer.clone();
            self.buffer.clear();
            self.ready = false;
            Some(cmd)
        } else {
            None
        }
    }
}

// Função para processar comandos
async fn process_command(cmd: &str, uart: &mut Uart<'static, embassy_stm32::mode::Async>) {
    let response = match cmd.trim() {
        "help" => "Comandos disponíveis:\r\n- help: Mostra esta ajuda\r\n- led on: Liga o LED\r\n- led off: Desliga o LED\r\n- led toggle: Alterna o LED\r\n- status: Mostra status do sistema\r\n",
        "led on" => {
            unsafe { LED_ENABLED = true; }
            "LED ligado\r\n"
        },
        "led off" => {
            unsafe { LED_ENABLED = false; }
            "LED desligado\r\n"
        },
        "led toggle" => {
            unsafe { LED_ENABLED = !LED_ENABLED; }
            if unsafe { LED_ENABLED } {
                "LED ligado\r\n"
            } else {
                "LED desligado\r\n"
            }
        },
        "status" => {
            if unsafe { LED_ENABLED } {
                "Sistema OK - LED ativo\r\n"
            } else {
                "Sistema OK - LED inativo\r\n"
            }
        },
        "" => "", // Comando vazio
        _ => "Comando não reconhecido. Digite 'help' para ajuda.\r\n",
    };

    if !response.is_empty() {
        uart.write(response.as_bytes()).await.unwrap();
    }
}

// Task do shell/terminal
#[embassy_executor::task]
async fn shell_task(mut uart: Uart<'static, embassy_stm32::mode::Async>) {
    let mut shell_cmd = ShellCommand::new();
    let mut buffer = [0u8; 1];

    // Mensagem de boas-vindas
    let welcome_msg = "\r\n=== STM32F407 Shell Terminal ===\r\n";
    uart.write(welcome_msg.as_bytes()).await.unwrap();
    let prompt_msg = "Digite 'help' para ver os comandos disponíveis.\r\nstm32> ";
    uart.write(prompt_msg.as_bytes()).await.unwrap();

    loop {
        // Lê um caractere
        uart.read(&mut buffer).await.unwrap();
        let received_char = buffer[0] as char;

        // Echo do caractere (exceto para caracteres especiais)
        if received_char.is_ascii_graphic() || received_char == ' ' {
            uart.write(&buffer).await.unwrap();
        } else if received_char == '\r' {
            uart.write("\r\n".as_bytes()).await.unwrap();
        } else if received_char == '\x08' || received_char == '\x7f' {
            // Backspace - remove caractere do terminal
            uart.write("\x08 \x08".as_bytes()).await.unwrap();
        }

        // Adiciona o caractere ao buffer de comando
        shell_cmd.add_char(received_char);

        // Verifica se o comando está pronto
        if let Some(command) = shell_cmd.get_command() {
            // Processa o comando
            process_command(&command, &mut uart).await;
            
            // Mostra o prompt novamente
            uart.write("stm32> ".as_bytes()).await.unwrap();
        }
    }
}

#[embassy_executor::task]
async fn button_task(mut button: ExtiInput<'static>) {
    info!("Press the USER button...");

    loop {
        button.wait_for_rising_edge().await;
        info!("Pressed!");
        if button.is_high() {
            info!("Pressed!");
            
            unsafe {
                LED_ENABLED = !LED_ENABLED;
            }
            
            button.wait_for_falling_edge().await;
            info!("Released!");
        }
    }
}

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
            divq: Some(PllQDiv::DIV7),
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
    let adc = Adc::new(p.ADC1);

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 2400;
    
    let usart = Uart::new(p.USART1, p.PA10, p.PA9, Irqs, p.DMA2_CH7, p.DMA2_CH2, uart_config).unwrap();

    // Spawn das tasks
    spawner.spawn(adc_task(adc, p.PA1.degrade_adc())).unwrap();
    spawner.spawn(button_task(button)).unwrap();
    spawner.spawn(shell_task(usart)).unwrap(); // Nova task do shell

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
                led1.set_low();
                Timer::after_millis(100).await;
            }
        }
    }
}