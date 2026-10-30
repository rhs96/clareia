# Clareia

Ferramentas pessoais para Windows, em Tauri 2 e Rust. [Baixar a versão mais recente](https://github.com/rhs96/clareia/releases/latest).

## Módulos

- **Rede:** bytes e taxas por processo com ETW, TCP/UDP, IPv4/IPv6. A lista padrão exige transferência no intervalo; sockets apenas abertos não contam como tráfego. Há histórico da sessão e filtro por protocolo. QUIC é contabilizado como UDP, sem identificação garantida do protocolo de aplicação.
- **Processos:** CPU, GPU pelo motor mais ocupado e memória. Encerramento com confirmação, proteção de processos críticos/do Windows e revalidação do PID com seu horário de criação.
- **Aplicativos:** inventário dos programas tradicionais registrados no Windows, desinstalador oficial e busca de pastas residuais por correspondência exata de nome em AppData e Temp.
- **Quarentena:** resíduos selecionados são movidos, com registro dos caminhos originais, e podem ser restaurados sem sobrescrita. Links e junções são rejeitados. Não há varredura nem remoção genérica de chaves do registro.
- **Scoop:** inventário do usuário atual, busca nos buckets instalados, instalação, atualização, desinstalação padrão e atualização dos catálogos. A pasta `persist` não é purgada automaticamente.
- **Windows Update:** consulta de atualizações de software da Microsoft, seleção e instalação via Windows Update Agent. Sem reinício automático. A instalação pode exigir elevação.
- **Atualizações do Clareia:** consulta ao GitHub Releases ao abrir e a cada 30 minutos; notificação em tela; instalação apenas após confirmação e verificação de assinatura.
- **Interface:** barra de título, botões de janela e scroll próprios. Não modifica o tema global do Windows.

## Uso

Prefira o instalador `Clareia_..._x64-setup.exe`. A versão portátil também está disponível, mas instalar oferece o caminho padrão para as próximas atualizações. Nenhum módulo instala software, encerra processos ou move arquivos sem uma confirmação na interface.

A contagem ETW por programa exige administrador. O botão no monitor reabre a aplicação com a autorização do Windows. Dados do histórico não são transferidos entre janelas. CPU, GPU e inventários podem continuar disponíveis sem essa permissão. A aplicação não pode confirmar nem descartar malware/mineração.

A coleta de rede fica local. Consultas de atualização e operações de instalação acessam GitHub, Microsoft e as origens dos buckets Scoop. Exportações podem conter IPs e caminhos: revise antes de compartilhar.

## Limites atuais

Não é um clone do Revo: a limpeza é conservadora, com revisão e quarentena, e não faz limpeza abrangente do registro nem rastreamento prévio de instaladores. O inventário Windows não inclui Store/MSIX. A busca Scoop depende dos buckets já instalados. Alguns desinstaladores com comandos interpretados não são executados por este painel.

Os bytes ETW são o campo `size` dos eventos de envio/recebimento, sem somar retransmissões TCP novamente. Incluem tráfego local e loopback. Não equivalem à cobrança do provedor; contadores de interfaces/VPN podem divergir. Até 64 fluxos por processo, 16.384 fluxos e 2.048 identidades por sessão; limitações e perdas são sinalizadas. GPU depende dos contadores do driver. O gráfico mantém 60 amostras e não há histórico persistente automático.

A quarentena usa `%LOCALAPPDATA%\Clareia\quarantine`. Não a apague se desejar restaurar arquivos. A busca de resíduos só é oferecida quando o registro de instalação deixa de existir após a desinstalação. Mesmo uma pasta com nome exato pode conter dados pessoais, por isso nada é selecionado automaticamente.

## Desenvolvimento

Windows, Node.js 24+, Rust MSVC, Visual Studio Build Tools C++ e WebView2.

```text
npm ci
npm test
cargo test --manifest-path src-tauri/Cargo.toml --release
npm run dev
npm run build:portable
```

O teste Rust de encerramento cria e encerra somente um processo do próprio executável de testes. O teste ignorado `sleeper` é usado como subprocesso por esse teste e não deve ser executado isoladamente.

## Publicar atualização

1. Atualize a versão em `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` e `src-tauri/tauri.conf.json`.
2. Faça commit e crie/envie uma tag `vX.Y.Z` correspondente.
3. O workflow Windows compila, testa, assina e publica o instalador e `latest.json` em Releases.

A chave privada está no secret `TAURI_SIGNING_PRIVATE_KEY`, não no repositório. A chave pública está na configuração Tauri. Guarde uma cópia privada da chave de assinatura: trocar a chave impede que versões antigas validem atualizações assinadas pela nova chave. Código alterado sem uma release de versão superior não dispara atualização para usuários.

## Fontes técnicas

- [Eventos TCP](https://learn.microsoft.com/en-us/windows/win32/etw/tcpip-typegroup1)
- [Eventos UDP](https://learn.microsoft.com/en-us/windows/win32/etw/udpip-typegroup1)
- [GPU no Windows](https://devblogs.microsoft.com/directx/gpus-in-the-task-manager/)
- [Atualizador Tauri](https://v2.tauri.app/plugin/updater/)
- [Comandos Scoop](https://github.com/ScoopInstaller/Scoop/wiki/Commands)
