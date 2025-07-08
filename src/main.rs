// Diretivas de compilação para sistema bare-metal (sem stdlib)
#![no_std]  // Não usar a biblioteca padrão do Rust
#![no_main] // Ponto de entrada personalizado (não é main())

// Importações de bibliotecas e módulos
use cortex_m_rt::pre_init; // Para código executado antes do main
use core::arch::asm;      // Para assembly inline
use defmt::*;            // Framework de logging para embedded
use embassy_executor::Spawner; // Executor assíncrono
use embassy_stm32::peripherals::ADC1; // Periférico ADC1
use embassy_stm32::time::Hertz; // Tipo para frequência
use embassy_stm32::Config; // Configuração do microcontrolador
use embassy_stm32::adc::{self, Adc, AdcChannel, AnyAdcChannel, SampleTime}; // ADC
use embassy_stm32::gpio::{Output, Pull, Level, Speed}; // GPIO
use embassy_stm32::interrupt; // Interrupções
use embassy_stm32::exti::ExtiInput; // Entrada com interrupção
use embassy_time::{Duration, Timer}; // Temporizador
use {defmt_rtt as _, panic_probe as _}; // Configuração de panic e logging
use embassy_stm32::bind_interrupts; // Vinculação de interrupções
use embassy_stm32::usart::{self, Uart}; // Comunicação serial
use heapless::String; // String de tamanho fixo (sem alocação dinâmica)
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use itoa; // Biblioteca para conversão de números inteiros em strings

// Variável global para controle do LED (acessada de forma unsafe)
static mut LED_ENABLED: bool = true;

static ADC_CHANNEL: Channel<ThreadModeRawMutex, u16, 10> = Channel::new();

// Task para leitura ADC
#[embassy_executor::task]
async fn adc_task(mut adc: adc::Adc<'static, ADC1>, mut adc_pin: AnyAdcChannel<ADC1>) {
    // Configura tempo de amostragem para 144 ciclos (balance entre velocidade e precisão)
    adc.set_sample_time(SampleTime::CYCLES144);

    loop {
        // Leitura bloqueante do valor ADC
        let measured = adc.blocking_read(&mut adc_pin);
        info!("measured: {}", measured); // Log do valor lido
        let _ = ADC_CHANNEL.try_send(measured);
        Timer::after_millis(500).await; // Espera 500ms entre leituras
    }
}

// Vinculação de interrupções para a USART1
bind_interrupts!(struct Irqs {
    USART1 => embassy_stm32::usart::InterruptHandler<embassy_stm32::peripherals::USART1>;
});

// Estrutura para armazenar e processar comandos do shell
struct ShellCommand {
    buffer: String<64>, // Buffer para armazenar o comando (tamanho máximo 64 bytes)
    ready: bool,       // Flag indicando se o comando está pronto para processamento
}

impl ShellCommand {
    // Cria uma nova instância do ShellCommand
    fn new() -> Self {
        Self {
            buffer: String::new(), // Buffer vazio
            ready: false,         // Comando não está pronto
        }
    }

    // Adiciona um caractere ao buffer de comando
    fn add_char(&mut self, c: char) {
        if c == '\r' || c == '\n' { // Enter - marca o comando como pronto
            self.ready = true;
        } else if c == '\x08' || c == '\x7f' { // Backspace - remove último caractere
            self.buffer.pop();
        } else if self.buffer.len() < 63 { // Adiciona caractere se houver espaço
            let _ = self.buffer.push(c);
        }
    }

    // Retorna o comando se estiver pronto
    fn get_command(&mut self) -> Option<String<64>> {
        if self.ready {
            let cmd = self.buffer.clone(); // Copia o comando
            self.buffer.clear(); // Limpa o buffer
            self.ready = false;  // Reseta a flag
            Some(cmd)           // Retorna o comando
        } else {
            None
        }
    }
}




// Função para processar comandos recebidos
async fn process_command(cmd: &str, uart: &mut Uart<'static, embassy_stm32::mode::Async>) {
    // Processa o comando e gera a resposta apropriada
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
        "adc cont" => {
    uart.write(b"Modo continuo (Ctrl+C para sair):\r\n").await.unwrap();
    
    let mut buf = [0u8; 1];
    loop {
        let value = ADC_CHANNEL.receive().await;
        uart.write(b"ADC: ").await.unwrap();
        uart.write(itoa::Buffer::new().format(value).as_bytes()).await.unwrap();
        uart.write(b"\r\n").await.unwrap();
        
        if uart.read_until_idle( &mut buf).await.is_ok() && buf[0] == 0x03 {
            break;
        }
    }
    
    uart.write(b"Modo continuo encerrado\r\n").await.unwrap();
    "" // Retorno compatível
},
        "" => "", // Comando vazio (não faz nada)
        _ => "Comando não reconhecido. Digite 'help' para ajuda.\r\n",
    };

    // Envia a resposta se não for vazia
    if !response.is_empty() {
        uart.write(response.as_bytes()).await.unwrap();
    }
}

// Task principal do shell/terminal
#[embassy_executor::task]
async fn shell_task(mut uart: Uart<'static, embassy_stm32::mode::Async>) {
    let mut shell_cmd = ShellCommand::new(); // Inicializa o processador de comandos
    let mut buffer = [0u8; 1]; // Buffer para leitura de um caractere por vez

    // Mensagem de boas-vindas
    let welcome_msg = "\r\n=== STM32F407 Shell Terminal ===\r\n";
    uart.write(welcome_msg.as_bytes()).await.unwrap();
    let prompt_msg = "Digite 'help' para ver os comandos disponíveis.\r\nstm32> ";
    uart.write(prompt_msg.as_bytes()).await.unwrap();

    loop {
        // Lê um caractere da UART
        uart.read(&mut buffer).await.unwrap();
        let received_char = buffer[0] as char;

        // Echo do caractere (exceto para caracteres especiais)
        if received_char.is_ascii_graphic() || received_char == ' ' {
            uart.write(&buffer).await.unwrap();
        } else if received_char == '\r' {
            uart.write("\r\n".as_bytes()).await.unwrap();
        } else if received_char == '\x08' || received_char == '\x7f' {
            // Backspace - remove caractere do terminal (espaço + backspace)
            uart.write("\x08 \x08".as_bytes()).await.unwrap();
        }

        // Adiciona o caractere ao buffer de comando
        shell_cmd.add_char(received_char);

        // Verifica se o comando está pronto para processamento
        if let Some(command) = shell_cmd.get_command() {
            // Processa o comando
            process_command(&command, &mut uart).await;
            
            // Mostra o prompt novamente
            uart.write("stm32> ".as_bytes()).await.unwrap();
        }
    }
}

// Task para tratamento do botão
#[embassy_executor::task]
async fn button_task(mut button: ExtiInput<'static>) {
    info!("Press the USER button...");

    loop {
        // Espera borda de subida (botão pressionado)
        button.wait_for_rising_edge().await;
        info!("Pressed!");
        
        if button.is_high() {
            info!("Pressed!");
            
            // Alterna estado do LED
            unsafe {
                LED_ENABLED = !LED_ENABLED;
            }
            
            // Espera borda de descida (botão solto)
            button.wait_for_falling_edge().await;
            info!("Released!");
        }
    }
}

// Variáveis em seções especiais de memória (CCMRAM e DATA2)
#[link_section = ".ccmram"]
static mut TESTE: i32 = 60;

#[link_section = ".data2"]
static mut TESTE2: i32 = 70;

// Função executada antes do main (pré-inicialização)
#[pre_init]
unsafe fn before_main() {
    // Assembly para inicializar a CCMRAM (Core Coupled Memory)
    unsafe {
        asm!{
            "ldr r0, =__sccmdata    // Início da CCMRAM
            ldr r1, =__eccmdata    // Fim da CCMRAM
            ldr r2, =__siccmdata   // Dados de inicialização
            0:
            cmp r1, r0             // Verifica se chegou ao fim
            beq 1f                 // Se sim, termina
            ldm r2!, {{r3}}        // Carrega dado da flash
            stm r0!, {{r3}}       // Armazena na CCMRAM
            b 0b                   // Repete
            1:"
        }

        // Assembly para inicializar a seção .data2
        asm!{
            "ldr r0, =__sdata2    // Início da seção
            ldr r1, =__edata2    // Fim da seção
            ldr r2, =__sidata2   // Dados de inicialização
            2:
            cmp r1, r0           // Verifica se chegou ao fim
            beq 3f               // Se sim, termina
            ldm r2!, {{r3}}      // Carrega dado da flash
            stm r0!, {{r3}}     // Armazena na RAM
            b 2b                // Repete
            3:"
        }
    }
}

// Função principal (executada após o pre_init)
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Habilita clock para GPIOC (unsafe pois acessa registrador diretamente)
    unsafe { embassy_stm32::pac::RCC.ahb1enr().modify(|r| r.set_gpiocen(true)); }
    
    // Configuração do sistema de clock
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000), // Cristal externo de 8MHz
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE; // Fonte do PLL é o HSE
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,    // Pré-divisor 4 (8MHz/4 = 2MHz)
            mul: PllMul::MUL168,       // Multiplica por 168 (2MHz*168 = 336MHz)
            divp: Some(PllPDiv::DIV2), // Divisor P (336MHz/2 = 168MHz - SYSCLK)
            divq: Some(PllQDiv::DIV7), // Divisor Q (para periféricos como USB)
            divr: Some(PllRDiv::DIV2), // Divisor R (para outros periféricos)
        });
        config.rcc.sys = Sysclk::PLL1_P; // Clock do sistema vem do PLL

        // Configura divisores de clock para os barramentos
        config.rcc.ahb_pre = AHBPrescaler::DIV1; // AHB a 168MHz
        config.rcc.apb1_pre = APBPrescaler::DIV4; // APB1 a 42MHz
        config.rcc.apb2_pre = APBPrescaler::DIV2; // APB2 a 84MHz
    }

    // Inicializa os periféricos com a configuração
    let p: embassy_stm32::Peripherals = embassy_stm32::init(config);

    info!("Hello World!"); // Mensagem inicial

    // Configuração dos periféricos:
    // - Botão com interrupção (PA0)
    let mut button = ExtiInput::new(p.PA0, p.EXTI0, Pull::Down);
    // - ADC1
    let adc = Adc::new(p.ADC1);

    // Configuração da UART (2400 baud, 8N1)
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 2400;
    
    // Inicializa a UART1 (TX=PA9, RX=PA10) com DMA
    let usart = Uart::new(
        p.USART1, 
        p.PA10, // RX
        p.PA9,  // TX
        Irqs,   // Interrupções
        p.DMA2_CH7, // DMA para TX
        p.DMA2_CH2, // DMA para RX
        uart_config
    ).unwrap();

    // Spawn das tasks assíncronas:
    // - Task do ADC (leitura contínua)
    spawner.spawn(adc_task(adc, p.PA1.degrade_adc())).unwrap();
    // - Task do botão (tratamento de interrupção)
    spawner.spawn(button_task(button)).unwrap();
    // - Task do shell (interface serial)
    spawner.spawn(shell_task(usart)).unwrap();

    // Configura LEDs como saídas (PD12 e PD13)
    let mut led1 = Output::new(p.PD12, Level::High, Speed::Low);
    let mut led2 = Output::new(p.PD13, Level::High, Speed::Low);

    // Loop principal - pisca o LED1 conforme o estado global
    loop {
        unsafe {
            if LED_ENABLED {
                // Pisca o LED a cada 500ms se habilitado
                led1.set_high();
                Timer::after_millis(500).await;
                led1.set_low();
                Timer::after_millis(500).await;
            } else {
                // Mantém LED desligado e espera 100ms
                led1.set_low();
                Timer::after_millis(100).await;
            }
        }
    }
}